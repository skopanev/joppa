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
    let mut unloaded = std::collections::BTreeSet::new();
    for row in client.query("SELECT s.section,s.key,(CASE WHEN s.section='objects' AND b.first_view->'object_types'->s.kind->>'inbox'='true' AND b.first_view->'object_types'->s.kind->>'role' IN ('group','document') THEN COALESCE(s.compact,s.value) ELSE s.value END)::text,(s.section='objects' AND b.first_view->'object_types'->s.kind->>'inbox'='true' AND b.first_view->'object_types'->s.kind->>'role' IN ('group','document')) FROM joppa.state_entries s JOIN joppa.board_cache b ON b.workspace=s.workspace WHERE s.workspace=$1 ORDER BY s.section,s.key", &[&workspace]).await? {
        let section: String = row.get(0);
        let key: String = row.get(1);
        let entry: Value = serde_json::from_str(row.get(2))?;
        if row.get::<_,Option<bool>>(3) == Some(true) { unloaded.insert(key.clone()); }
        if section == "meta" {
            for (key, value_entry) in entry.as_object().unwrap() {
                value[key] = value_entry.clone();
            }
        } else {
            if !value[&section].is_object() { value[&section] = json!({}); }
            value[&section][key] = entry;
        }
    }
    let mut state: Workspace = serde_json::from_value(value)?;
    state.unloaded_objects = unloaded;
    ensure!(
        !state.admin.is_empty(),
        "current projection is missing; run backfill-board"
    );
    Ok((state, position.get::<_, i64>(0) as u64))
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
    for row in client.query("SELECT key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key=ANY($2)", &[&workspace,&addresses]).await? {
        let key: String=row.get(0);
        state.objects.insert(key.clone(),serde_json::from_str(row.get(1))?);
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
    if matches!(
        command["op"].as_str(),
        Some(
            "claim_task"
                | "release_task"
                | "block_task"
                | "unblock_task"
                | "complete_task"
                | "review_task"
                | "retire_task"
                | "amend_task"
        )
    ) && let Some(id) = command["id"].as_str()
    {
        addresses.push(crate::objects::resolve_address(state, id).to_owned());
    }
    if matches!(
        command["op"].as_str(),
        Some("claim_planning" | "finish_planning" | "release_planning")
    ) && let Some(ac) = command["ac"].as_str()
    {
        let ac = crate::objects::resolve_address(state, ac).to_owned();
        addresses.push(ac.clone());
        if command["op"] == "release_planning" {
            addresses.extend(
                state
                    .task_process
                    .keys()
                    .filter(|key| {
                        state
                            .objects
                            .get(*key)
                            .is_some_and(|o| o.current().parent.as_deref() == Some(ac.as_str()))
                    })
                    .cloned(),
            );
        }
    }
    if let Some(target) = command["target"].as_str() {
        addresses.push(crate::objects::resolve_address(state, target).to_owned());
    }
    if command["op"] == "finish_planning"
        && let Some(ac) = command["ac"].as_str()
    {
        let ac = crate::objects::resolve_address(state, ac);
        if let Some(claim) = state.planning_claims.get(ac) {
            let keys: Vec<String> = state
                .task_process
                .keys()
                .filter(|key| {
                    state.objects.get(*key).is_some_and(|o| {
                        o.current()
                            .parent
                            .as_deref()
                            .map(|p| crate::objects::resolve_address(state, p))
                            == Some(ac)
                    })
                })
                .cloned()
                .collect();
            // A blocked Task also needs its document revision at the block position.
            let (blocked, positions): (Vec<String>, Vec<i64>) = keys
                .iter()
                .filter_map(|key| {
                    let block = state.task_process[key].blocked.as_ref()?;
                    Some((key.clone(), block.position as i64))
                })
                .unzip();
            for row in client.query("SELECT address,document::text FROM joppa.object_revisions WHERE workspace=$1 AND address=ANY($2) AND ((document->>'position')::bigint>$3 OR (address,(document->>'position')::bigint) IN (SELECT * FROM unnest($4::text[],$5::bigint[]))) ORDER BY address,revision", &[&workspace, &keys, &(claim.position as i64), &blocked, &positions]).await? {
                    let key: String = row.get(0);
                    let revision: crate::objects::ObjectRevision = serde_json::from_str(row.get(1))?;
                    let object = state.objects.get_mut(&key).context("unknown Task")?;
                    if !object.revisions.iter().any(|r| r.revision == revision.revision) { object.revisions.push(revision); }
                    object.revisions.sort_by_key(|r| r.revision);
                }
        }
    }
    hydrate_objects(client, workspace, state, &addresses).await?;
    hydrate_objects(client, workspace, previous, &addresses).await?;
    Ok(())
}
