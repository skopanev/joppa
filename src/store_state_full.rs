//! Part of `store_state`: the full state with every revision, and its journal.
use super::*;

/// Explicit full snapshots include history without executing journal commands. The events of
/// the journal stream into the list that the caller reads.
pub(crate) async fn full(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<(Workspace, Vec<Event>)> {
    let (state, _) = full_state(client, workspace).await?;
    let mut events = vec![];
    let sql = "SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position";
    super::stream::each(client, sql, &[&workspace], |row| {
        let actor: String = row.get(1);
        let (position, recorded_at) = (row.get::<_, i64>(0) as u64, row.get(2));
        let command = serde_json::from_str(row.get(3))?;
        events.push(Event {
            position,
            actor,
            recorded_at,
            command,
        });
        Ok(())
    })
    .await?;
    Ok((state, events))
}

/// The full state with every revision and the last journal position, without the events.
pub(crate) async fn full_state(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<(Workspace, u64)> {
    let (mut state, _) = current(client, workspace).await?;
    let unloaded: Vec<String> = state.unloaded_objects.iter().cloned().collect();
    hydrate_objects(client, workspace, &mut state, &unloaded).await?;
    let sql = "SELECT section,key,value::text FROM joppa.state_revisions WHERE workspace=$1 ORDER BY section,key,revision";
    super::stream::each(client, sql, &[&workspace], |row| {
        let (section, key): (&str, String) = (row.get(0), row.get(1));
        match section {
            "requirements" => {
                let r: crate::model::Revision = serde_json::from_str(row.get(2))?;
                if let Some(q) = state
                    .requirements
                    .get_mut(&key)
                    .map(std::sync::Arc::make_mut)
                {
                    q.revisions.retain(|old| old.number != r.number);
                    q.revisions.push(r);
                    q.revisions.sort_by_key(|r| r.number);
                }
            }
            "tree_schema" => {
                let r: crate::objects::SchemaRevision = serde_json::from_str(row.get(2))?;
                state.tree_schema.retain(|old| old.revision != r.revision);
                state.tree_schema.push(r);
                state.tree_schema.sort_by_key(|r| r.revision);
            }
            _ => {}
        }
        Ok(())
    })
    .await?;
    let sql = "SELECT address,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision";
    super::stream::each(client, sql, &[&workspace], |row| {
        let key: String = row.get(0);
        let r: crate::objects::ObjectRevision = serde_json::from_str(row.get(1))?;
        if let Some(o) = state.objects.get_mut(&key).map(std::sync::Arc::make_mut) {
            o.revisions.retain(|old| old.revision != r.revision);
            o.revisions.push(r);
            o.revisions.sort_by_key(|r| r.revision);
        }
        Ok(())
    })
    .await?;
    let sql = "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1";
    let position: i64 = client.query_one(sql, &[&workspace]).await?.get(0);
    Ok((state, position as u64))
}
