use crate::{
    checks,
    commands::{self, Command},
    model::Workspace,
};
use anyhow::{Context, Result, ensure};
use deadpool_postgres::Pool;
use serde::Serialize;
use serde_json::{Value, json};
use tokio_postgres::GenericClient;
#[path = "store_access.rs"]
mod access;
pub use access::{
    AccessDenied, enroll, enroll_granted, grant_domain, grant_email, granted_workspaces,
    list_domain_grants, list_email_grants, list_owned_access, remove_member, revoke_domain,
    revoke_email, update_identity_name,
};
#[path = "store_board.rs"]
mod board;
pub use board::board_read;
#[path = "store_objects.rs"]
mod objects;
pub use objects::{
    ObjectScope, ProjectionNotReady, compact_list, link_page, read_objects, tree_schema,
};
#[path = "store_grants.rs"]
mod grants;
#[path = "store_queue.rs"]
mod queue_rows;
pub use grants::{
    Access, READ_ONLY_REFUSAL, TokenGrant, create_agent_token, list_agent_tokens,
    revoke_agent_token, token_actor,
};
#[path = "store_record_results.rs"]
mod record_results;
#[path = "store_reports.rs"]
mod reports;
pub use record_results::record_results;
pub use reports::{criterion_tests, task_status, tests_overview};
#[path = "store_tasks.rs"]
mod tasks;
pub use tasks::{task_queue, task_queue_filtered};
#[path = "store_remote.rs"]
pub(crate) mod remote;
#[path = "store_remote_status.rs"]
pub(crate) mod remote_status;
#[path = "store_timeline.rs"]
mod timeline;
#[path = "store_timeline_items.rs"]
mod timeline_items;
#[path = "store_waits.rs"]
mod waits;
pub use timeline::{TimelineScope, read_timeline};
#[path = "store_history.rs"]
mod history;
#[path = "store_references.rs"]
mod references;
pub use history::{HISTORY_LIMIT, mark_history, read_history};
pub use references::workspace_references;

pub const DEFAULT_URL: &str = "postgresql://postgres:joppa@127.0.0.1:55433/joppa";
#[path = "store_setup.rs"]
mod setup;
pub use setup::{
    backfill_board, check_journal, migrate, migrate_notes, pool, refresh_readiness_metadata,
};
async fn ensure_notes(
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
            Command::RenameWorkspace {
                workspace_id: None,
                title: state.title.clone(),
                reason: workspace_prefix::REASON.into(),
                reference_prefix: prefix.clone(),
            },
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
/// A change of objects::work_ready, objects::process or objects::signal must raise this
/// version, so backfill_board reprojects every workspace at serve start.
pub(super) const BOARD_PROJECTION_VERSION: &str = "14";
pub(super) const READINESS_METADATA_VERSION: &str = "2";

#[derive(Serialize)]
pub struct Event {
    pub position: u64,
    pub actor: String,
    pub actor_type: String,
    pub recorded_at: String,
    pub command: Command,
}
async fn load(client: &impl GenericClient, workspace: &str) -> Result<(Workspace, Vec<Event>)> {
    let rows = client.query("SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position", &[&workspace]).await.context("run joppa migrate before using the journal")?;
    let mut state = Workspace::default();
    if !rows.is_empty() {
        state.replay_epochs = replay_epochs(client, workspace).await?;
    }
    let mut events = vec![];
    for row in rows {
        let position = row.get::<_, i64>(0) as u64;
        let actor: String = row.get(1);
        let command: Command = serde_json::from_str(row.get::<_, &str>(3))?;
        commands::apply_replay(&mut state, &actor, &command, position, row.get(2))
            .with_context(|| format!("invalid journal entry at {workspace}/{position}"))?;
        let actor_type = crate::inbox::actor_type(&state, &actor).to_owned();
        events.push(Event {
            position,
            actor,
            actor_type,
            recorded_at: row.get(2),
            command,
        });
    }
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

/// A new workspace starts with every replay rule in force.
pub(super) async fn start_replay_epochs(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<()> {
    for rule in commands::REPLAY_RULES {
        client
            .execute(
                "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) VALUES($1,$2,1)",
                &[&workspace, &rule],
            )
            .await?;
    }
    Ok(())
}
async fn resolve_workspace(client: &impl GenericClient, name: &str) -> Result<String> {
    Ok(client
        .query_opt(
            "SELECT workspace FROM joppa.workspace_aliases WHERE name=$1",
            &[&name],
        )
        .await?
        .map(|r| r.get(0))
        .unwrap_or_else(|| name.to_owned()))
}
async fn reserve_name(client: &impl GenericClient, name: &str, workspace: &str) -> Result<()> {
    client.execute("INSERT INTO joppa.workspace_aliases(name,workspace) VALUES($1,$2) ON CONFLICT DO NOTHING", &[&name,&workspace]).await?;
    ensure!(
        resolve_workspace(client, name).await? == workspace,
        "workspace ID is already in use"
    );
    Ok(())
}
#[path = "store_state.rs"]
mod state_rows;
#[path = "store_write.rs"]
mod write;
pub use write::{
    BatchOperation, check_fields, enroll_service, execute, execute_batch, execute_input,
    execute_remote_batch, execute_remote_input, save_dependency,
};
fn summaries(state: &Workspace) -> Value {
    let summaries: serde_json::Map<String, Value> = state
        .requirements
        .iter()
        .map(|(id, q)| {
            (
                id.clone(),
                crate::objects::requirement_summary(state, id, q.revisions.last().unwrap()),
            )
        })
        .collect();
    Value::Object(summaries)
}
#[path = "store_read.rs"]
mod read_scope;
pub use read_scope::{Scope, read};

#[path = "store_items.rs"]
mod items;
pub use items::{gate, list};

#[path = "search.rs"]
pub mod search;
#[path = "search_indexer.rs"]
pub mod search_indexer;
#[path = "search_query.rs"]
pub mod search_query;
#[path = "search_revisions.rs"]
mod search_revisions;
#[path = "workspace_prefix.rs"]
pub mod workspace_prefix;

#[path = "store_blobs.rs"]
mod blobs;
pub use blobs::{ATTACHMENT_MAX, get_blob, put_blob};
