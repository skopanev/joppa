//! Part of `store_state`: `full` and the items after it, moved from store_state.rs unchanged.
use super::*;

/// Explicit full snapshots include history without executing journal commands.
pub(crate) async fn full(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<(Workspace, Vec<Event>)> {
    let (mut state, _) = current(client, workspace).await?;
    let unloaded: Vec<String> = state.unloaded_objects.iter().cloned().collect();
    hydrate_objects(client, workspace, &mut state, &unloaded).await?;
    for row in client.query("SELECT section,key,value::text FROM joppa.state_revisions WHERE workspace=$1 ORDER BY section,key,revision", &[&workspace]).await? {
        let section: String = row.get(0);
        let key: String = row.get(1);
        match section.as_str() {
            "requirements" => {
                let r: crate::model::Revision = serde_json::from_str(row.get(2))?;
                if let Some(q) = state.requirements.get_mut(&key) {
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
    }
    for row in client.query("SELECT address,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision", &[&workspace]).await? {
        let key: String = row.get(0);
        let r: crate::objects::ObjectRevision = serde_json::from_str(row.get(1))?;
        if let Some(o) = state.objects.get_mut(&key) {
            o.revisions.retain(|old| old.revision != r.revision);
            o.revisions.push(r);
            o.revisions.sort_by_key(|r| r.revision);
        }
    }
    let mut events = vec![];
    for row in client.query("SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position", &[&workspace]).await? {
        let actor: String = row.get(1);
        let actor_type = crate::inbox::actor_type(&state, &actor).to_owned();
        events.push(Event{position:row.get::<_,i64>(0) as u64,actor,actor_type,recorded_at:row.get(2),command:serde_json::from_str(row.get(3))?});
    }
    Ok((state, events))
}
