//! The helpers of the save_object tests of deploy 24 over a real server (record_world.rs):
//! writes of the agent bot, the journal, the stored revisions and a journal replay.
#![allow(dead_code)]
use super::record_world::World;
use anyhow::Result;
use serde_json::{Value, json};

/// One command of the agent bot through joppa_command.
pub async fn save(w: &World, command: Value) -> Result<Result<Value, String>> {
    w.command(&w.bot, command).await
}

/// A command of the agent bot that must succeed.
pub async fn ok(w: &World, command: Value) -> Result<Value> {
    let text = command.to_string();
    save(w, command)
        .await?
        .map_err(|e| anyhow::anyhow!("{text}: {e}"))
}

/// One joppa_batch of the agent bot with the commands `commands`, in their order.
pub async fn batch(w: &World, commands: &[Value]) -> Result<Result<Value, String>> {
    let operations: Vec<Value> = commands.iter().map(|c| json!({"command":c})).collect();
    let request_id = uuid::Uuid::new_v4().to_string();
    let args = json!({"request_id":request_id,"operations":operations});
    w.mcp(&w.bot, "joppa_batch", args).await
}

/// The position of the newest journal event of the workspace, or 0.
pub async fn head(w: &World) -> Result<i64> {
    let sql = "SELECT COALESCE(MAX(position),0)::bigint FROM joppa.events WHERE workspace=$1";
    Ok(w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0))
}

/// The stored commands of the journal after the position `after`, in their order.
pub async fn events_after(w: &World, after: i64) -> Result<Vec<Value>> {
    let sql = "SELECT command::text FROM joppa.events WHERE workspace=$1 AND position>$2 ORDER BY position";
    let rows = w.pool.get().await?.query(sql, &[&w.ws, &after]).await?;
    rows.iter()
        .map(|r| Ok(serde_json::from_str(r.get(0))?))
        .collect()
}

/// Whether the workspace has a current object at `address`.
pub async fn exists(w: &World, address: &str) -> Result<bool> {
    let sql = "SELECT count(*) FROM joppa.objects WHERE workspace=$1 AND address=$2";
    let row = w
        .pool
        .get()
        .await?
        .query_one(sql, &[&w.ws, &address])
        .await?;
    Ok(row.get::<_, i64>(0) > 0)
}

/// The public item ID of one object of the workspace.
pub async fn item_id(w: &World, kind: &str, id: &str) -> Result<String> {
    let sql = "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind=$2 AND local_id=$3";
    let row = w
        .pool
        .get()
        .await?
        .query_one(sql, &[&w.ws, &kind, &id])
        .await?;
    Ok(row.get(0))
}

/// Each stored revision of `address` as (revision, document), in the order of the revisions.
pub async fn revisions(w: &World, address: &str) -> Result<Vec<(i64, Value)>> {
    let sql = "SELECT revision::bigint,document::text FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 ORDER BY revision";
    let rows = w.pool.get().await?.query(sql, &[&w.ws, &address]).await?;
    rows.iter()
        .map(|r| Ok((r.get(0), serde_json::from_str(r.get(1))?)))
        .collect()
}

/// Each current document and each stored revision document of the workspace, in order.
pub async fn documents(w: &World) -> Result<Vec<(String, i64, Value)>> {
    let sql = "SELECT address,-1::bigint,document::text FROM joppa.objects WHERE workspace=$1 UNION ALL SELECT address,revision::bigint,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY 1,2";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    rows.iter()
        .map(|r| Ok((r.get(0), r.get(1), serde_json::from_str(r.get(2))?)))
        .collect()
}

/// Rebuild every projection of the workspace from its journal, as a serve start does for a
/// stale board.
pub async fn replay(w: &World) -> Result<()> {
    let sql = "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1";
    w.pool.get().await?.execute(sql, &[&w.ws]).await?;
    joppa::store::backfill_board(&w.pool).await?;
    Ok(())
}

/// Append `command` of `actor` as one journal event, without the live checks. The caller
/// stops the server first and starts it again after the append, which replays the journal.
pub async fn append_event(w: &World, actor: &str, command: &Value) -> Result<()> {
    let sql = "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,COALESCE(MAX(position),0)+1,$2,$3,$4::text::jsonb FROM joppa.events WHERE workspace=$1";
    let request = uuid::Uuid::new_v4().to_string();
    let text = command.to_string();
    let conn = w.pool.get().await?;
    conn.execute(sql, &[&w.ws, &actor, &request, &text]).await?;
    joppa::store::backfill_board(&w.pool).await?;
    Ok(())
}

/// A refusal that contains `error`.
pub fn refused(reply: Result<Value, String>, error: &str) -> Result<()> {
    anyhow::ensure!(
        reply.as_ref().is_err_and(|e| e.contains(error)),
        "refusal with {error:?} expected: {reply:?}"
    );
    Ok(())
}
