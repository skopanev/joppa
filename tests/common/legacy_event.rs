//! A write of a test fixture: a live operation through the store, and an operation that live
//! writes refuse as one event of an old journal, which the projections rebuild from.
use joppa::store;
use serde_json::Value;

/// Write `command` as `actor`. A removed operation gives a null reply.
pub async fn execute(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    command: Value,
) -> anyhow::Result<Value> {
    let request = uuid::Uuid::new_v4().to_string();
    let op = command["op"].as_str().unwrap_or("");
    if !joppa::removed_ops::NAMES.contains(&op) {
        return store::execute_input(pool, ws, actor, &request, &command).await;
    }
    pool.get().await?.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,COALESCE(MAX(position),0)+1,$2,$3,$4::text::jsonb FROM joppa.events WHERE workspace=$1",
        &[&ws, &actor, &request, &command.to_string()],
    ).await?;
    store::backfill_board(pool).await?;
    Ok(Value::Null)
}
