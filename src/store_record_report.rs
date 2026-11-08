//! The journal event of a record of test results. A record appends a record_report event,
//! so the journal order holds every recorded change and History lists the record.
use super::*;

/// Append the record_report event of one accepted record. The caller holds the
/// transaction; a failed append rolls back the record.
pub(super) async fn record_report(
    tx: &impl GenericClient,
    workspace: &str,
    actor: &str,
    command: Command,
) -> Result<u64> {
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&workspace],
    )
    .await?;
    let (mut state, base_position) = state_rows::current(tx, workspace).await?;
    state.alias = crate::actor_alias::current();
    let previous = state.clone();
    let position = base_position + 1;
    let recorded_at: String = tx
        .query_one(
            "INSERT INTO joppa.events(workspace,position,request_id,actor,command,alias) VALUES($1,$2,$3,$4,$5::text::jsonb,$6) RETURNING recorded_at::text",
            &[
                &workspace,
                &(position as i64),
                &format!("report-{}", uuid::Uuid::new_v4()),
                &actor,
                &serde_json::to_string(&command)?,
                &state.alias,
            ],
        )
        .await?
        .get(0);
    commands::apply(&mut state, actor, &command, position, &recorded_at)?;
    let events = [Event {
        position,
        actor: actor.into(),
        recorded_at,
        command,
    }];
    let selected = state_rows::affected(&state, Some(&previous))?;
    search::project(tx, workspace, &state, position, selected.as_ref()).await?;
    let at = (events.as_slice(), position);
    board::project_selected(tx, workspace, &state, at, Some(&previous), selected).await?;
    Ok(position)
}
