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

pub const DEFAULT_URL: &str = "postgresql://postgres:joppa@127.0.0.1:55433/joppa";
pub fn pool(url: &str) -> Result<Pool> {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(url.to_string());
    Ok(cfg.create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?)
}
pub async fn migrate(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    tx.batch_execute(include_str!("../migrations/001_journal.sql"))
        .await?;
    tx.commit().await?;
    Ok(())
}
#[derive(Serialize)]
pub struct Event {
    pub position: u64,
    pub actor: String,
    pub recorded_at: String,
    pub command: Command,
}
async fn load(client: &impl GenericClient, workspace: &str) -> Result<(Workspace, Vec<Event>)> {
    let rows = client.query("SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position", &[&workspace]).await.context("run joppa migrate before using the journal")?;
    let mut state = Workspace::default();
    let mut events = vec![];
    for row in rows {
        let position = row.get::<_, i64>(0) as u64;
        let actor: String = row.get(1);
        let command: Command = serde_json::from_str(row.get::<_, &str>(3))?;
        commands::apply(&mut state, &actor, &command, position)
            .with_context(|| format!("invalid journal entry at {workspace}/{position}"))?;
        events.push(Event {
            position,
            actor,
            recorded_at: row.get(2),
            command,
        });
    }
    Ok((state, events))
}
pub async fn execute(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    command: &Command,
) -> Result<Value> {
    commands::identifier(workspace)?;
    commands::identifier(request_id)?;
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    // Serializes creation too. Hash collisions only delay unrelated workspaces.
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&workspace],
    )
    .await?;
    let (mut state, events) = load(&*tx, workspace).await?;
    let payload = serde_json::to_string(command)?;
    if let Some(row) = tx.query_opt("SELECT actor,command::text,position FROM joppa.events WHERE workspace=$1 AND request_id=$2", &[&workspace, &request_id]).await? {
        ensure!(row.get::<_, String>(0) == actor && serde_json::from_str::<Value>(row.get::<_, &str>(1))? == serde_json::to_value(command)?, "request_id reused with different content/actor");
        return Ok(json!({"position": row.get::<_, i64>(2), "replayed": true}));
    }
    let position = events.len() as u64 + 1;
    commands::apply(&mut state, actor, command, position)?;
    if position == 1 {
        tx.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&workspace])
            .await?;
    }
    tx.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb)", &[&workspace, &(position as i64), &actor, &request_id, &payload]).await?;
    tx.commit().await?;
    Ok(json!({"position": position, "replayed": false, "summary": summaries(&state)}))
}
fn summaries(state: &Workspace) -> Value {
    let summaries: serde_json::Map<String, Value> = state
        .requirements
        .iter()
        .map(|(id, q)| (id.clone(), checks::summary(q.revisions.last().unwrap())))
        .collect();
    Value::Object(summaries)
}
pub async fn read(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let (state, events) = load(&**conn, workspace).await?;
    ensure!(
        state.members.contains_key(actor),
        "workspace not found or access denied"
    );
    let summaries = summaries(&state);
    let configured = state.requirements.values().any(|q| {
        q.revisions
            .last()
            .unwrap()
            .acs
            .iter()
            .any(|a| a.before_launch)
    });
    let launch_blockers: Vec<_> = state
        .requirements
        .iter()
        .flat_map(|(id, q)| {
            let r = q.revisions.last().unwrap();
            let accepted = summaries[id]["accepted"] == true;
            r.acs
                .iter()
                .filter(move |a| a.before_launch && !accepted)
                .map(move |a| json!({"req": id, "revision": r.number, "ac": a.id}))
        })
        .collect();
    Ok(
        json!({"workspace": workspace, "position": events.len(), "state": state, "summary": summaries, "history": events, "launch": {"ready": configured && launch_blockers.is_empty(), "configured": configured, "blockers": launch_blockers, "meaning": "Only explicitly marked before_launch ACs are evaluated; this is not a deployment command."}}),
    )
}
pub async fn list(pool: &Pool, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let rows = conn
        .query("SELECT id FROM joppa.workspaces ORDER BY id", &[])
        .await?;
    let mut items = vec![];
    for row in rows {
        let id: String = row.get(0);
        let (w, _) = load(&**conn, &id).await?;
        if w.members.contains_key(actor) {
            items.push(json!({"id": id, "title": w.title}));
        }
    }
    Ok(json!(items))
}

/// The deployment caller supplies the exact target. Accepted staging evidence
/// must never authorize a different production build or provider configuration.
pub fn gate(card: &Value, subject: &std::collections::BTreeMap<String, String>) -> Result<()> {
    ensure!(
        card["launch"]["ready"] == true,
        "launch conditions are absent or not accepted"
    );
    let state: Workspace = serde_json::from_value(card["state"].clone())?;
    for q in state.requirements.values() {
        let r = q.revisions.last().unwrap();
        if !r.acs.iter().any(|a| a.before_launch) {
            continue;
        }
        let (_, run) = checks::latest(r).ok_or_else(|| anyhow::anyhow!("no verified subject"))?;
        ensure!(
            run.subject
                .iter()
                .all(|(key, value)| subject.get(key) == Some(value)),
            "launch target differs from the verified subject for {}",
            r.title
        );
    }
    Ok(())
}
