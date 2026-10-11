use anyhow::{Result, ensure};
use serde_json::json;

/// Set fixture event time before insertion. Keep the journal and current rows consistent.
pub async fn age(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    address: &str,
    minutes: i64,
) -> Result<()> {
    let client = pool.get().await?;
    let database: String = client
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    client.batch_execute(r#"
        CREATE OR REPLACE FUNCTION public.joppa_test_claim_time() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN
            IF NEW.request_id LIKE 'test-claim-time-%' THEN
                NEW.recorded_at := clock_timestamp() - make_interval(mins => split_part(NEW.request_id,'-',4)::integer);
            END IF;
            RETURN NEW;
        END $$;
        DO $$ BEGIN
            IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname='joppa_test_claim_time' AND tgrelid='joppa.events'::regclass) THEN
                CREATE TRIGGER joppa_test_claim_time BEFORE INSERT ON joppa.events
                    FOR EACH ROW EXECUTE FUNCTION public.joppa_test_claim_time();
            END IF;
        END $$;
    "#).await?;
    let row=client.query_one("SELECT revision,process->>'assignee' FROM joppa.objects WHERE workspace=$1 AND address=$2", &[&workspace,&address]).await?;
    let revision: i32 = row.get(0);
    let holder: String = row
        .get::<_, Option<String>>(1)
        .ok_or_else(|| anyhow::anyhow!("fixture Task must have a holder"))?;
    drop(client);
    let request = format!(
        "test-claim-time-{minutes}-{}",
        uuid::Uuid::new_v4().simple()
    );
    joppa::store::execute_remote_input(
        pool,
        workspace,
        &holder,
        &request,
        &json!({"op":"heartbeat_task","id":address,"revision":revision}),
    )
    .await?;
    Ok(())
}
