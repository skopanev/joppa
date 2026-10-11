use anyhow::{Result, ensure};
use serde_json::json;

/// Change only current claim metadata in an isolated test database.
pub async fn age(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    address: &str,
    minutes: i64,
) -> Result<()> {
    let mut client = pool.get().await?;
    let database: String = client
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    let tx = client.transaction().await?;
    let now: i64 = tx
        .query_one(
            "SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint",
            &[],
        )
        .await?
        .get(0);
    let heartbeat = now - minutes * 60_000;
    let deadline = heartbeat + joppa::tasks::CLAIM_DURATION_MS;
    let claim = json!({"heartbeat_at_ms":heartbeat,"deadline_ms":deadline}).to_string();
    ensure!(tx.execute("UPDATE joppa.state_entries SET value=jsonb_set(value,'{claim}',$3::text::jsonb) WHERE workspace=$1 AND section='task_process' AND key=$2", &[&workspace,&address,&claim]).await? == 1, "unknown fixture Task");
    tx.execute("UPDATE joppa.objects SET process=jsonb_set(process,'{claim}',$3::text::jsonb) WHERE workspace=$1 AND address=$2", &[&workspace,&address,&claim]).await?;
    tx.execute("UPDATE joppa.task_queue SET claim_deadline_ms=$3,data=jsonb_set(data,'{claim}',$4::text::jsonb) WHERE workspace=$1 AND address=$2", &[&workspace,&address,&deadline,&claim]).await?;
    tx.commit().await?;
    Ok(())
}
