//! Part of `store_state`: `current` and the items after it, moved from store_state.rs unchanged.
use super::*;

/// Read current rows. Historical revisions and journal payloads stay in PostgreSQL.
pub(crate) async fn current(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<(Workspace, u64)> {
    let position = client
        .query_opt(
            "SELECT position FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    let Some(position) = position else {
        ensure!(
            !client
                .query_one(
                    "SELECT EXISTS(SELECT 1 FROM joppa.workspaces WHERE id=$1)",
                    &[&workspace]
                )
                .await?
                .get::<_, bool>(0),
            "current projection is missing; run backfill-board"
        );
        return Ok((Workspace::default(), 0));
    };
    let mut value = serde_json::to_value(Workspace::default())?;
    // The large sections become their maps at once, row by row, without a JSON value of the
    // whole workspace. Only meta and the small sections fold into `value`.
    let mut sections = Sections::default();
    crate::probe::count(&crate::probe::STATE_LOADS);
    // The compact inbox types come from the board row once, not once for each row, and the
    // rows need no order: each one goes into a map. The summaries are not workspace state.
    let sql = "WITH compact AS MATERIALIZED (SELECT t.key AS kind FROM joppa.board_cache b CROSS JOIN LATERAL jsonb_each(b.first_view->'object_types') t WHERE b.workspace=$1 AND t.value->>'inbox'='true' AND t.value->>'role' IN ('group','document') AND COALESCE(t.value->>'source','false')<>'true') SELECT s.section,s.key,(CASE WHEN c.kind IS NOT NULL THEN COALESCE(s.compact,s.value) ELSE s.value END)::text,c.kind IS NOT NULL FROM joppa.state_entries s LEFT JOIN compact c ON s.section='objects' AND c.kind=s.kind WHERE s.workspace=$1 AND s.section<>'summaries'";
    let rows = client.query_raw(sql, [&workspace]).await?;
    futures_util::pin_mut!(rows);
    while let Some(row) = futures_util::TryStreamExt::try_next(&mut rows).await? {
        let (section, key, text): (&str, String, &str) = (row.get(0), row.get(1), row.get(2));
        if row.get::<_, bool>(3) {
            sections.unloaded.insert(key.clone());
        }
        if !sections.take(section, &key, text)? {
            fold(&mut value, section, key, serde_json::from_str(text)?);
        }
    }
    let mut state: Workspace = serde_json::from_value(value)?;
    sections.into(&mut state);
    ensure!(
        !state.admin.is_empty(),
        "current projection is missing; run backfill-board"
    );
    Ok((state, position.get::<_, i64>(0) as u64))
}

#[path = "store_state_sections.rs"]
mod sections;
use sections::Sections;

/// The sets of addresses that keep one row for each address, not a list in the meta row.
const SETS: [&str; 2] = ["document_overrides", "archived_objects"];

/// Put one state row other than an object into the JSON value of a workspace.
fn fold(value: &mut Value, section: &str, key: String, entry: Value) {
    if section == "meta" {
        for (key, value_entry) in entry.as_object().into_iter().flatten() {
            // A meta row before migration 070 holds a set as a list: the lists merge.
            match (value[key].as_array_mut(), value_entry.as_array()) {
                (Some(list), Some(more)) if SETS.contains(&key.as_str()) => {
                    list.extend(more.iter().cloned())
                }
                _ => value[key] = value_entry.clone(),
            }
        }
    } else if SETS.contains(&section) {
        if !value[section].is_array() {
            value[section] = json!([]);
        }
        value[section]
            .as_array_mut()
            .unwrap()
            .push(Value::String(key));
    } else {
        if !value[section].is_object() {
            value[section] = json!({});
        }
        value[section][key] = entry;
    }
}

/// The workspace without objects: only the meta row and the tree order. It gives the schema,
/// the members and the order of siblings, and reads no object row.
pub(crate) async fn outline(client: &impl GenericClient, workspace: &str) -> Result<Workspace> {
    let mut value = serde_json::to_value(Workspace::default())?;
    let sql = "SELECT section,key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section IN ('meta','tree_order')";
    for row in client.query(sql, &[&workspace]).await? {
        let section: String = row.get(0);
        fold(
            &mut value,
            &section,
            row.get(1),
            serde_json::from_str(row.get(2))?,
        );
    }
    Ok(serde_json::from_value(value)?)
}

pub(crate) async fn hydrate_objects(
    client: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    addresses: &[String],
) -> Result<()> {
    let addresses: Vec<String> = addresses
        .iter()
        .filter(|key| state.unloaded_objects.contains(*key))
        .cloned()
        .collect();
    if addresses.is_empty() {
        return Ok(());
    }
    let sql = "SELECT key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key=ANY($2)";
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 2] = [&workspace, &addresses];
    let rows = client.query_raw(sql, params).await?;
    futures_util::pin_mut!(rows);
    while let Some(row) = futures_util::TryStreamExt::try_next(&mut rows).await? {
        let key: String = row.get(0);
        state
            .objects
            .insert(key.clone(), serde_json::from_str(row.get(1))?);
        state.unloaded_objects.remove(&key);
    }
    Ok(())
}

pub(crate) async fn hydrate_command(
    client: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    previous: &mut Workspace,
    command: &Value,
) -> Result<()> {
    let mut addresses = vec![];
    if command["op"] == "set_tree_schema" {
        addresses.extend(state.unloaded_objects.iter().cloned());
    }
    if let (Some(kind), Some(id)) = (command["kind"].as_str(), command["id"].as_str()) {
        addresses.push(format!("{kind}:{id}"));
    }
    if let Some(target) = command["target"].as_str() {
        addresses.push(crate::objects::resolve_address(state, target).to_owned());
    }
    hydrate_objects(client, workspace, state, &addresses).await?;
    hydrate_objects(client, workspace, previous, &addresses).await?;
    Ok(())
}
