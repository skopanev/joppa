// Part 1 of tests/search_step_postgres.rs: enrollment queue marker check (ac:5lrja53z).

// ac:5lrja53z
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn enrollment_by_grant_queues_move_marker_not_full_rebuild() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    setup_domain_schema(&w).await?;
    // Bring search_state up to current position P and empty search_pending.
    w.mcp(&w.bot, "joppa_search", json!({"query":"R"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let conn = w.pool.get().await?;
    let p: i64 = conn
        .query_one("SELECT position FROM joppa.search_state WHERE workspace=$1", &[&w.ws])
        .await?
        .get(0);
    let pending_before: i64 = conn
        .query_one("SELECT count(*) FROM joppa.search_pending WHERE workspace=$1", &[&w.ws])
        .await?
        .get(0);
    ensure!(pending_before == 0, "pending not empty before enrollment");

    // Grant email to maria and enroll maria (event P+1).
    let maria = format!("google-maria-{}", uuid::Uuid::new_v4().simple());
    let email = format!("{maria}@example.com");
    conn.execute(
        "INSERT INTO joppa.google_identities(actor,google_sub,email) VALUES($1,$1,$2)",
        &[&maria, &email],
    )
    .await?;
    store::grant_email(&w.pool, &w.ws, "owner", &email).await?;
    store::enroll_granted(&w.pool, &maria, None, None).await?;

    let after_enroll: Vec<(String, i64)> = conn
        .query(
            "SELECT address, position FROM joppa.search_pending WHERE workspace=$1 ORDER BY id",
            &[&w.ws],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    ensure!(
        after_enroll == vec![("#".to_owned(), p + 1)],
        "after enroll got {after_enroll:?}"
    );

    // Save domain:x (event P+2) directly through store so we inspect search_pending before background drain.
    store::execute_remote_input(
        &w.pool,
        &w.ws,
        "bot",
        "save-x",
        &json!({"op":"save_object","kind":"domain","id":"x","revision":0,"parent":null,"data":{"title":"Domain X","owner":"owner"},"reason":"Create"}),
    )
    .await?;
    let after_save: Vec<(String, i64)> = conn
        .query(
            "SELECT address, position FROM joppa.search_pending WHERE workspace=$1 ORDER BY id",
            &[&w.ws],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    ensure!(
        !after_save.iter().any(|(a, _)| a == "*"),
        "search_pending held *: {after_save:?}"
    );
    ensure!(
        after_save == vec![("#".to_owned(), p + 1), ("domain:x".to_owned(), p + 2)],
        "after save got {after_save:?}"
    );

    let found = w
        .mcp(&w.bot, "joppa_search", json!({"query":"Domain X"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        found["items"]
            .as_array()
            .context("items")?
            .iter()
            .any(|i| i["address"] == "domain:x"),
        "{found}"
    );
    let row = conn
        .query_one(
            "SELECT (SELECT position FROM joppa.search_state WHERE workspace=$1), (SELECT count(*) FROM joppa.search_pending WHERE workspace=$1)",
            &[&w.ws],
        )
        .await?;
    let (state_pos, left): (i64, i64) = (row.get(0), row.get(1));
    ensure!(state_pos == p + 2 && left == 0, "state_pos={state_pos}, left={left}");
    Ok(())
}
