//! Part of `store`: the replay of the journal of one workspace.
use super::*;

/// A change of the tree schema or a migration of every node: the projection rebuilds all.
pub(in crate::store) fn schema_event(command: &Command) -> bool {
    matches!(
        command,
        Command::SetTreeSchema(crate::commands::SetTreeSchemaOp { .. })
            | Command::MigrateSchema {}
            | Command::MigrateDocuments {}
            | Command::MigrateConsumers {}
            | Command::MigrateTestRules {}
            | Command::MigrateNoteParents {}
            | Command::MigrateBody {}
            | Command::MigratePurpose {}
            | Command::MigrateTestNodes {}
            | Command::MigrateGenericNodes {}
            | Command::MigrateDecisionRequired {}
    )
}
/// The replay of a journal. The events stream and apply one at a time. The projection of a
/// replay reads only the schema events, the redactions and the last event, so only these
/// events stay in the list, the last event included.
pub(super) async fn load(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<(Workspace, Vec<Event>)> {
    crate::probe::count(&crate::probe::REPLAY);
    let sql = "SELECT EXISTS(SELECT 1 FROM joppa.events WHERE workspace=$1)";
    let row = client.query_one(sql, &[&workspace]).await;
    let any: bool = row
        .context("run joppa migrate before using the journal")?
        .get(0);
    let mut state = Workspace::default();
    if any {
        state.replay_epochs = replay_epochs(client, workspace).await?;
    }
    let (mut events, mut last) = (vec![], None);
    let sql = "SELECT position,actor,recorded_at::text,command::text,alias FROM joppa.events WHERE workspace=$1 ORDER BY position";
    stream::each(client, sql, &[&workspace], |row| {
        let position = row.get::<_, i64>(0) as u64;
        let actor: String = row.get(1);
        let command: Command = serde_json::from_str(row.get::<_, &str>(3))?;
        state.alias = row.get(4);
        commands::apply_replay(&mut state, &actor, &command, position, row.get(2))
            .with_context(|| format!("invalid journal entry at {workspace}/{position}"))?;
        let kept = schema_event(&command) || matches!(command, Command::RedactObject { .. });
        let event = Event {
            position,
            actor,
            recorded_at: row.get(2),
            command,
        };
        if kept {
            events.push(event);
            last = None;
        } else {
            last = Some(event);
        }
        Ok(())
    })
    .await?;
    events.extend(last);
    Ok((state, events))
}
/// The replay epochs of a workspace that has a journal. A missing rule stops the replay:
/// without its epoch the replay cannot tell the old meaning of an event from the new one.
async fn replay_epochs(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<std::collections::BTreeMap<String, u64>> {
    let epochs: std::collections::BTreeMap<String, u64> = client
        .query(
            "SELECT rule,first_position FROM joppa.replay_epochs WHERE workspace=$1",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|row| (row.get::<_, String>(0), row.get::<_, i64>(1) as u64))
        .collect();
    for rule in commands::REPLAY_RULES {
        ensure!(
            epochs.contains_key(rule),
            "workspace {workspace} has no replay epoch for rule {rule}; run joppa migrate"
        );
    }
    Ok(epochs)
}
