//! The status projection. Every write stores the test status of every object and the row
//! decoration of every tree node in joppa.objects, in its own transaction. A read uses the
//! stored rows and computes nothing. A stored row from an older code version stays readable
//! until the background catch-up (store_backfill.rs) rebuilds it.
use super::*;
use std::collections::BTreeMap;

/// The code version of the stored statuses and decorations. A change of the status rules or
/// of the decoration helpers must raise it.
pub(in crate::store) const STATUS_PROJECTION_VERSION: i32 = 9;

/// The row fields that the decoration helpers set, at the top level and inside process.
const FIELDS: [&str; 3] = ["work_ready", "waits_for", "child_counts"];
const PROCESS_FIELDS: &[&str] = &["verification"];
const SIGNAL_FIELDS: &[&str] = &["tone", "label"];

/// Rebuild the projection of a workspace. The caller has written the objects and the board
/// rows of `position` in the same transaction, under the workspace lock.
pub(in crate::store) async fn rebuild(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    position: u64,
) -> Result<()> {
    crate::probe::count(&crate::probe::REBUILD);
    let types = crate::objects::schema(state).types;
    let addresses: Vec<String> = state.objects.keys().cloned().collect();
    let statuses =
        super::reports::state_criterion_statuses(client, workspace, state, &addresses).await?;
    let rows: Vec<Value> = statuses
        .iter()
        .map(|(address, status)| json!({"address":address,"value":status}))
        .collect();
    store_column(client, workspace, "test_status", &rows).await?;
    stamp(client, workspace, position).await?;
    // A new object has no rollup yet, and the helpers below pass the ready check. This
    // placeholder lives only inside this transaction: the last step stores every rollup.
    client
        .execute(
            "UPDATE joppa.objects SET rollup='{}'::jsonb WHERE workspace=$1 AND rollup IS NULL",
            &[&workspace],
        )
        .await?;
    // The helpers below read the statuses that were just stored. Each base row has the
    // fields that the helpers read, with the same values as a list row.
    let mut items: Vec<Value> = client
        .query(
            "SELECT address,kind,signal::text,process::text,work_ready,parent,present FROM joppa.objects WHERE workspace=$1",
            &[&workspace],
        )
        .await?
        .iter()
        .map(base_row)
        .collect();
    let base = items.clone();
    let counted = add_status_tones(client, workspace, &mut items, &types).await?;
    add_child_counts(&mut items, &counted, &types);
    let (waits, counts) = super::waits::read_with_counts(client, workspace, &types).await?;
    add_waits(client, workspace, &mut items, &waits, &counts).await?;
    super::ready_rollup::roll_ready(&mut items, &counted, &types);
    let patches: Vec<Value> = base.iter().zip(&items).map(|(b, a)| patch(b, a)).collect();
    store_column(client, workspace, "rollup", &patches).await?;
    super::dimensions::project(client, workspace, state).await
}

/// The stored rollup of one row: each decoration field that differs from the base row. An
/// empty patch is stored as {}, so a NULL rollup always means a row that is not projected.
pub(super) fn patch(before: &Value, after: &Value) -> Value {
    let mut patch = serde_json::Map::new();
    for field in FIELDS {
        if before.get(field) != after.get(field) {
            patch.insert(field.into(), after[field].clone());
        }
    }
    for (part, fields) in [("process", PROCESS_FIELDS), ("signal", SIGNAL_FIELDS)] {
        let mut inner = serde_json::Map::new();
        for field in fields {
            if before[part].get(field) != after[part].get(field) {
                inner.insert((*field).into(), after[part][field].clone());
            }
        }
        if !inner.is_empty() {
            patch.insert(part.into(), Value::Object(inner));
        }
    }
    json!({"address":after["address"],"value":patch})
}

/// Mark the status projection current at `position` with this code version.
pub(super) async fn stamp(
    client: &impl GenericClient,
    workspace: &str,
    position: u64,
) -> Result<()> {
    client.execute("UPDATE joppa.board_cache SET first_view=first_view||jsonb_build_object('status_projection',jsonb_build_object('version',$2::int,'position',$3::bigint)) WHERE workspace=$1", &[&workspace, &STATUS_PROJECTION_VERSION, &(position as i64)]).await?;
    Ok(())
}

/// The base row of a projected object: the fields that the decoration helpers read, with the
/// same values as a list row, from address, kind, signal, process, work_ready, parent, present.
pub(super) fn base_row(r: &tokio_postgres::Row) -> Value {
    let parse = |i: usize| {
        r.get::<_, Option<String>>(i)
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    };
    json!({"address":r.get::<_,String>(0),"kind":r.get::<_,String>(1),
        "signal":parse(2).unwrap_or_else(|| json!({})),"process":parse(3).unwrap_or(Value::Null),
        "work_ready":r.get::<_,bool>(4),"parent":r.get::<_,Option<String>>(5),
        "present":r.get::<_,bool>(6)})
}

/// The child counts of one node: for each type of its present children with a tone, in the
/// order of the type index, how many there are and how many are green.
pub(super) fn child_counts<'a>(
    children: impl IntoIterator<Item = (&'a str, &'a str)>,
    types: &BTreeMap<String, objects::TreeType>,
) -> Option<Value> {
    let mut by_kind = BTreeMap::<&str, (u64, u64)>::new();
    for (child, tone) in children {
        let Some((kind, _)) = child.split_once(':') else {
            continue;
        };
        let count = by_kind.entry(kind).or_default();
        count.0 += 1;
        count.1 += u64::from(tone == "green");
    }
    let mut counts: Vec<_> = by_kind.into_iter().collect();
    counts.sort_by_key(|(kind, _)| (types.get(*kind).and_then(|t| t.index), *kind));
    (!counts.is_empty()).then(|| {
        let counts = counts
            .iter()
            .map(|(kind, (total, passed))| json!({"kind":kind,"total":total,"passed":passed}));
        json!(counts.collect::<Vec<_>>())
    })
}

/// The child counts of every node of a full rebuild.
fn add_child_counts(
    items: &mut [Value],
    counted: &super::tone_items::Counted,
    types: &BTreeMap<String, objects::TreeType>,
) {
    for item in items.iter_mut() {
        let Some(address) = item["address"].as_str() else {
            continue;
        };
        let children = counted.children.get(address).into_iter().flatten();
        let toned = children.filter_map(|c| Some((c.as_str(), (*counted.tones.get(c)?)?)));
        if let Some(counts) = child_counts(toned, types) {
            item["child_counts"] = counts;
        }
    }
}

/// Set the stored decoration on each row, as the helpers set it at the last write.
pub(in crate::store) async fn apply_rollups(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
) -> Result<()> {
    let addresses: Vec<String> = items
        .iter()
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect();
    let patches: BTreeMap<String, Value> = client
        .query(
            "SELECT address,rollup::text FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)",
            &[&workspace, &addresses],
        )
        .await?
        .iter()
        .map(|r| Ok((r.get(0), stored(r.get(1))?)))
        .collect::<Result<_>>()?;
    for item in items {
        let Some(patch) = item["address"].as_str().and_then(|a| patches.get(a)) else {
            continue;
        };
        for field in FIELDS {
            if let Some(value) = patch.get(field) {
                item[field] = value.clone();
            }
        }
        for part in ["process", "signal"] {
            if let Some(inner) = patch[part].as_object()
                && item[part].is_object()
            {
                for (field, value) in inner {
                    item[part][field] = value.clone();
                }
            }
        }
    }
    Ok(())
}

/// The stored statuses of the requested objects. Objects without a stored status are left
/// out, as the live read left out addresses without an object row.
pub(in crate::store) async fn stored_statuses(
    client: &impl GenericClient,
    workspace: &str,
    targets: &[String],
) -> Result<BTreeMap<String, Value>> {
    client
        .query(
            "SELECT address,test_status::text FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)",
            &[&workspace, &targets],
        )
        .await?
        .iter()
        .map(|r| Ok((r.get(0), stored(r.get(1))?)))
        .collect()
}

/// Store one value per object in `column`, and clear it on every other object of the
/// workspace. Only rows whose value changes are written.
pub(super) async fn store_column(
    client: &impl GenericClient,
    workspace: &str,
    column: &str,
    rows: &[Value],
) -> Result<()> {
    client.execute(&format!("UPDATE joppa.objects o SET {column}=x.value FROM (SELECT p.address,v.value FROM joppa.objects p LEFT JOIN jsonb_to_recordset($2::text::jsonb) v(address text,value jsonb) ON v.address=p.address WHERE p.workspace=$1) x WHERE o.workspace=$1 AND o.address=x.address AND o.{column} IS DISTINCT FROM x.value"), &[&workspace, &json!(rows).to_string()]).await?;
    Ok(())
}

/// Store one value for each listed object in `column`; every other row keeps its value.
pub(super) async fn store_rows(
    client: &impl GenericClient,
    workspace: &str,
    column: &str,
    rows: &[Value],
) -> Result<()> {
    client.execute(&format!("UPDATE joppa.objects o SET {column}=x.value FROM jsonb_to_recordset($2::text::jsonb) x(address text,value jsonb) WHERE o.workspace=$1 AND o.address=x.address AND o.{column} IS DISTINCT FROM x.value"), &[&workspace, &json!(rows).to_string()]).await?;
    Ok(())
}

/// A stored value. A row that the status projection has not reached yet has none, and a
/// read shows it without decoration until the background catch-up stores it.
fn stored(text: Option<String>) -> Result<Value> {
    text.map_or(Ok(json!({})), |text| Ok(serde_json::from_str(&text)?))
}
