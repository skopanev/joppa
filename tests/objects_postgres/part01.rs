// Part 1 of tests/objects_postgres.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn objects_replay_concurrency_ids_and_schema_are_atomic() -> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let name: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    drop(conn);
    store::migrate(&pool).await?;
    let ws = format!(
        "objects-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    // Projection writes must work with the production role's restricted grants.
    let conn = pool.get().await?;
    conn.batch_execute("DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='joppa_test_projection_guard') THEN CREATE ROLE joppa_test_projection_guard; END IF; END $$; GRANT USAGE ON SCHEMA joppa TO joppa_test_projection_guard; GRANT SELECT,INSERT ON ALL TABLES IN SCHEMA joppa TO joppa_test_projection_guard; GRANT UPDATE ON joppa.objects,joppa.board_cache,joppa.state_revisions,joppa.state_entries,joppa.search_documents,joppa.search_state,joppa.search_revisions TO joppa_test_projection_guard; GRANT DELETE ON joppa.state_entries,joppa.task_queue TO joppa_test_projection_guard; GRANT DELETE ON joppa.object_links,joppa.search_documents,joppa.search_state TO joppa_test_projection_guard; GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA joppa TO joppa_test_projection_guard;").await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_projection_no_rewrite() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'projection updated an unchanged row'; END $$;").await?;
    let statement = format!(
        "CREATE TRIGGER test_projection_no_rewrite BEFORE UPDATE ON joppa.objects FOR EACH ROW WHEN (OLD.workspace='{}' AND OLD.address='domain:d') EXECUTE FUNCTION joppa.test_projection_no_rewrite()",
        ws
    );
    conn.batch_execute(&statement).await?;
    drop(conn);
    let restricted_url = format!(
        "{}{}options=-c%20role%3Djoppa_test_projection_guard",
        std::env::var("JOPPA_TEST_DATABASE_URL")?,
        if std::env::var("JOPPA_TEST_DATABASE_URL")?.contains('?') {
            "&"
        } else {
            "?"
        }
    );
    replay_concurrency_ids_and_schema_after_setup(&pool, &ws, &restricted_url).await
}
