//! The status projection. Every write stores the test status of every object and the row
//! decoration of every tree node in joppa.objects, in its own transaction. A read uses the stored rows and
//! computes nothing. A missing or stale projection is an explicit error, not an old status.
use super::*;
use std::collections::BTreeMap;

/// The code version of the stored statuses and decorations. A change of the status rules or
/// of the decoration helpers must raise it.
pub(in crate::store) const STATUS_PROJECTION_VERSION: i32 = 2;

/// A read found no projection, or one from an older journal position or code version.
#[derive(Debug)]
pub struct ProjectionNotReady;

impl std::fmt::Display for ProjectionNotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("criterion status projection is not ready")
    }
}

impl std::error::Error for ProjectionNotReady {}

/// The row fields that the decoration helpers set, at the top level and inside process.
const FIELDS: [&str; 4] = [
    "work_ready",
    "waits_for",
    "verification_summary",
    "value_tones",
];
const PROCESS_FIELDS: [&str; 2] = ["verification", "readiness_blocker"];
const SIGNAL_FIELDS: [&str; 2] = ["tone", "label"];

/// Refuse a read unless the projection matches the board position and the code version,
/// and every object of the workspace has a stored status and decoration.
pub(in crate::store) async fn ensure_ready(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<()> {
    let row = client
        .query_opt(
            "SELECT (first_view->'status_projection'->>'version')::int,(first_view->'status_projection'->>'position')::bigint,position,EXISTS(SELECT 1 FROM joppa.objects o WHERE o.workspace=$1 AND (o.test_status IS NULL OR o.rollup IS NULL)) FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    match row {
        Some(row)
            if row.get::<_, Option<i32>>(0) == Some(STATUS_PROJECTION_VERSION)
                && row.get::<_, Option<i64>>(1) == Some(row.get::<_, i64>(2))
                // A row without a stored value is a partial projection.
                && !row.get::<_, bool>(3) =>
        {
            Ok(())
        }
        _ => Err(ProjectionNotReady.into()),
    }
}

/// Rebuild the projection of a workspace. The caller has written the objects and the board
/// rows of `position` in the same transaction, under the workspace lock.
pub(in crate::store) async fn rebuild(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    position: u64,
) -> Result<()> {
    crate::probe::count(&crate::probe::REBUILD);
    let types = crate::objects::ordered_types(&crate::objects::schema(state));
    let addresses: Vec<String> = state.objects.keys().cloned().collect();
    let statuses =
        super::reports::state_criterion_statuses(client, workspace, state, &addresses).await?;
    let rows: Vec<Value> = statuses
        .iter()
        .map(|(address, status)| json!({"address":address,"value":status}))
        .collect();
    store_column(client, workspace, "test_status", &rows).await?;
    client.execute("UPDATE joppa.board_cache SET first_view=first_view||jsonb_build_object('status_projection',jsonb_build_object('version',$2::int,'position',$3::bigint)) WHERE workspace=$1", &[&workspace, &STATUS_PROJECTION_VERSION, &(position as i64)]).await?;
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
            "SELECT address,kind,signal::text,process::text,work_ready,parent FROM joppa.objects WHERE workspace=$1",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| {
            let parse = |i: usize| {
                r.get::<_, Option<String>>(i)
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            };
            json!({"address":r.get::<_,String>(0),"kind":r.get::<_,String>(1),
                "signal":parse(2).unwrap_or_else(|| json!({})),"process":parse(3).unwrap_or(Value::Null),
                "work_ready":r.get::<_,bool>(4),"parent":r.get::<_,Option<String>>(5)})
        })
        .collect();
    let base = items.clone();
    let criteria: Vec<String> = types
        .iter()
        .filter(|(_, t)| t.role == "ac")
        .map(|(kind, _)| kind.clone())
        .collect();
    add_verification_summaries(client, workspace, &mut items, &types).await?;
    add_status_tones(client, workspace, &mut items, &types, None).await?;
    super::value_tones::add(client, workspace, state, &base, &mut items, &types).await?;
    let (waits, counts) = super::waits::read_with_counts(client, workspace, &types).await?;
    add_waits(client, workspace, &mut items, &waits, &counts, &criteria).await?;
    let patches: Vec<Value> = base
        .iter()
        .zip(&items)
        .map(|(before, after)| {
            let mut patch = serde_json::Map::new();
            for field in FIELDS {
                if before.get(field) != after.get(field) {
                    patch.insert(field.into(), after[field].clone());
                }
            }
            for (part, fields) in [("process", &PROCESS_FIELDS), ("signal", &SIGNAL_FIELDS)] {
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
            // An empty patch is stored as {}, so a NULL rollup always means a lost row.
            json!({"address":after["address"],"value":patch})
        })
        .collect();
    store_column(client, workspace, "rollup", &patches).await?;
    Ok(())
}

/// Set the stored decoration on each row, as the helpers set it at the last write.
pub(in crate::store) async fn apply_rollups(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
) -> Result<()> {
    ensure_ready(client, workspace).await?;
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

/// The stored waits: each waiting node with its first unsatisfied target.
pub(in crate::store) async fn projected_waits(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<BTreeMap<String, String>> {
    crate::probe::count(&crate::probe::WAIT_READS);
    ensure_ready(client, workspace).await?;
    client
        .query(
            "SELECT address,(rollup->'waits_for'->>'address'),rollup IS NULL FROM joppa.objects WHERE workspace=$1 AND (rollup IS NULL OR rollup ? 'waits_for')",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| match r.get::<_, bool>(2) {
            true => Err(ProjectionNotReady.into()),
            false => Ok((r.get(0), r.get(1))),
        })
        .collect::<Result<_>>()
}

/// The stored statuses of the requested objects. Objects without a stored status are left
/// out, as the live read left out addresses without an object row.
pub(in crate::store) async fn stored_statuses(
    client: &impl GenericClient,
    workspace: &str,
    targets: &[String],
) -> Result<BTreeMap<String, Value>> {
    ensure_ready(client, workspace).await?;
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
async fn store_column(
    client: &impl GenericClient,
    workspace: &str,
    column: &str,
    rows: &[Value],
) -> Result<()> {
    client.execute(&format!("UPDATE joppa.objects o SET {column}=x.value FROM (SELECT p.address,v.value FROM joppa.objects p LEFT JOIN jsonb_to_recordset($2::text::jsonb) v(address text,value jsonb) ON v.address=p.address WHERE p.workspace=$1) x WHERE o.workspace=$1 AND o.address=x.address AND o.{column} IS DISTINCT FROM x.value"), &[&workspace, &json!(rows).to_string()]).await?;
    Ok(())
}

/// A stored value. Every rebuild writes a value for every object, so NULL is a lost row.
fn stored(text: Option<String>) -> Result<Value> {
    let text = text.ok_or(ProjectionNotReady)?;
    Ok(serde_json::from_str(&text)?)
}
