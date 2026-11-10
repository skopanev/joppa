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
        (!state.body_migrated, Command::MigrateBody {}),
        (!state.purpose_migrated, Command::MigratePurpose {}),
        (!state.test_nodes_migrated, Command::MigrateTestNodes {}),
        (!state.generic_nodes, Command::MigrateGenericNodes {}),
        (
            !state.decision_required_migrated,
            Command::MigrateDecisionRequired {},
        ),
        (
            prefix.is_some(),
            Command::UpdateWorkspace(crate::commands::UpdateWorkspaceOp {
                reason: Some(workspace_prefix::REASON.into()),
                reference_prefix: prefix.clone(),
                ..Default::default()
            }),
        ),
    ] {
        if !needed {
            continue;
        }
        ensure!(!state.admin.is_empty(), "workspace has no creator");
        let position = events
            .last()
            .map_or(base_position + 1, |event| event.position + 1);
        // The creator is the system actor. A human member may remove the creator, and only the
        // prefix update can come after that: then the first human member sends it.
        let actor = if commands::is_human_member(state, &state.admin) {
            state.admin.clone()
        } else {
            state
                .members
                .iter()
                .find(|(_, role)| *role == "human")
                .map_or_else(|| state.admin.clone(), |(id, _)| id.clone())
        };
        let payload = serde_json::to_string(&command)?;
        let recorded_at: String = client.query_one("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb) RETURNING recorded_at::text", &[&workspace,&(position as i64),&actor,&uuid::Uuid::new_v4().to_string(),&payload]).await?.get(0);
        // A server migration has no alias, also after a replay that ends with one.
        state.alias = None;
        commands::apply_replay(state, &actor, &command, position, &recorded_at)?;
        // The new test nodes get their public item IDs, as a saved object does.
        if matches!(command, Command::MigrateTestNodes {}) {
            let prefix = state.reference_prefix.as_deref().unwrap_or(workspace);
            for o in state
                .objects
                .values()
                .filter(|o| crate::test_nodes::kinds(state).contains(&o.kind))
            {
                client
                    .execute(
                        "SELECT joppa.register_item($1,$2,$3,$4)",
                        &[&workspace, &o.kind, &o.id, &prefix],
                    )
                    .await?;
            }
        }
        events.push(Event {
            position,
            actor,
            recorded_at,
            command,
        });
        changed = true;
    }
    Ok(changed)
}
