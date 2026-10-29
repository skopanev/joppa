//! The migration events that a write appends before its own events.
use super::*;

pub(super) async fn ensure_notes(
    client: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    events: &mut Vec<Event>,
    base_position: u64,
) -> Result<bool> {
    let mut changed = false;
    let prefix = workspace_prefix::wanted(client, workspace, state).await?;
    for (needed, command) in [
        (!state.notes_migrated, Command::MigrateNotes {}),
        (!state.tasks_migrated, Command::MigrateTasks {}),
        (!state.schema_migrated, Command::MigrateSchema {}),
        (!state.documents_migrated, Command::MigrateDocuments {}),
        (!state.claims_migrated, Command::MigrateTaskClaims {}),
        (
            !state.persistent_claims,
            Command::MigratePersistentClaims {},
        ),
        (!state.consumers_migrated, Command::MigrateConsumers {}),
        (
            !state.consumer_drops_noted,
            Command::MigrateConsumerDrops {},
        ),
        (!state.test_rules_migrated, Command::MigrateTestRules {}),
        (!state.note_parents_migrated, Command::MigrateNoteParents {}),
        (
            prefix.is_some(),
            Command::RenameWorkspace(crate::commands::RenameWorkspaceOp {
                workspace_id: None,
                title: state.title.clone(),
                reason: workspace_prefix::REASON.into(),
                reference_prefix: prefix.clone(),
            }),
        ),
    ] {
        if !needed {
            continue;
        }
        ensure!(!state.admin.is_empty(), "workspace has no owner");
        let position = events
            .last()
            .map_or(base_position + 1, |event| event.position + 1);
        let actor = state.admin.clone();
        let payload = serde_json::to_string(&command)?;
        let recorded_at: String = client.query_one("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb) RETURNING recorded_at::text", &[&workspace,&(position as i64),&actor,&uuid::Uuid::new_v4().to_string(),&payload]).await?.get(0);
        commands::apply_replay(state, &actor, &command, position, &recorded_at)?;
        let actor_type = crate::inbox::actor_type(state, &actor).to_owned();
        events.push(Event {
            position,
            actor,
            actor_type,
            recorded_at,
            command,
        });
        changed = true;
    }
    Ok(changed)
}
