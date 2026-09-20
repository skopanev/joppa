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
pub use access::{AccessDenied, enroll};

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
    tx.batch_execute(include_str!("../migrations/002_item_ids.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/003_reference_prefix.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/004_cli_credentials.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/005_mcp_oauth.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/006_workspace_aliases.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/007_command_receipts.sql"))
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
#[path = "store_write.rs"]
mod write;
pub use write::{BatchOperation, execute, execute_batch, execute_input};
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
    let resolved = resolve_workspace(&**conn, workspace).await?;
    let workspace = resolved.as_str();
    access::check(&**conn, workspace, actor, false).await?;
    let (state, events) = load(&**conn, workspace).await?;
    ensure!(
        state.members.contains_key(actor),
        "workspace not found or access denied"
    );
    let mut system_ids = json!({"domain":{},"capability":{},"requirement":{},"ac":{}});
    for row in conn
        .query(
            "SELECT kind,local_id,id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2",
            &[
                &workspace,
                &state.reference_prefix.as_deref().unwrap_or(workspace),
            ],
        )
        .await?
    {
        let kind: String = row.get(0);
        let local: String = row.get(1);
        let id: String = row.get(2);
        system_ids[&kind][&local] = json!(id);
    }
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
        json!({"workspace": state.workspace_id.as_deref().unwrap_or(workspace), "system_ids":system_ids, "position": events.len(), "state": state, "summary": summaries, "history": events, "launch": {"ready": configured && launch_blockers.is_empty(), "configured": configured, "blockers": launch_blockers, "meaning": "Only explicitly marked before_launch ACs are evaluated; this is not a deployment command."}}),
    )
}
pub async fn list(pool: &Pool, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let allowed = access::scope(&**conn, actor).await?;
    let rows = conn
        .query(
            &format!("{} ORDER BY workspace", access::MEMBERSHIP),
            &[&actor],
        )
        .await?;
    let mut items = vec![];
    for row in rows {
        let id: String = row.get(0);
        if allowed.as_ref().is_some_and(|set| !set.contains(&id)) {
            continue;
        }
        let (w, _) = load(&**conn, &id).await?;
        if w.members.contains_key(actor) {
            items.push(json!({"id": w.workspace_id.as_deref().unwrap_or(&id), "title": w.title}));
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

/// Resolve a globally unique reference without exposing another workspace's content.
pub async fn read_item(pool: &Pool, id: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let row = conn
        .query_opt(
            "SELECT workspace,kind,local_id FROM joppa.item_ids WHERE id=$1",
            &[&id],
        )
        .await?
        .ok_or(AccessDenied)?;
    let workspace: String = row.get(0);
    let kind: String = row.get(1);
    let local_id: String = row.get(2);
    drop(conn);
    let mut result = read(pool, &workspace, actor).await?;
    result["selection"] = json!({"kind":kind,"id":local_id,"system_id":id});
    if kind == "ac" {
        let state: Workspace = serde_json::from_value(result["state"].clone())?;
        for (req, q) in &state.requirements {
            if let Some(revision) = q
                .revisions
                .iter()
                .rev()
                .find(|r| r.acs.iter().any(|ac| ac.id == local_id))
            {
                result["selection"]["req"] = json!(req);
                result["selection"]["revision"] = json!(revision.number);
                break;
            }
        }
    }
    Ok(result)
}
