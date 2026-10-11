use anyhow::{Result, ensure};

/// Set fixture event time before insertion. Keep the journal and current rows consistent.
pub async fn install(pool: &deadpool_postgres::Pool) -> Result<()> {
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
                NEW.recorded_at := clock_timestamp() - split_part(NEW.request_id,'-',4)::bigint * interval '1 millisecond';
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
    Ok(())
}
