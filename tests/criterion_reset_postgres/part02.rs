// Part 2 of tests/criterion_reset_postgres.rs, included there, so the test names stay unchanged.

// check-list-frozen-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failed_boundary_write_rejects_the_group_change() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    let events = |pool: deadpool_postgres::Pool, ws: String| async move {
        Ok::<i64, anyhow::Error>(
            pool.get()
                .await?
                .query_one(
                    "SELECT count(*) FROM joppa.events WHERE workspace=$1",
                    &[&ws],
                )
                .await?
                .get(0),
        )
    };
    let before = snapshot(&pool, &ws).await?;
    let count = events(pool.clone(), ws.clone()).await?;
    let conn = pool.get().await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_reset_fails() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'boundary write failed'; END $$;").await?;
    conn.batch_execute(&format!("CREATE TRIGGER test_reset_fails BEFORE INSERT OR UPDATE ON joppa.state_entries FOR EACH ROW WHEN (NEW.workspace='{ws}' AND NEW.section='criterion_boundaries') EXECUTE FUNCTION joppa.test_reset_fails()")).await?;
    let g = read(&pool, &ws, "area:g").await?["current_revision"].clone();
    let result = write(&pool, &ws, "owner", save_g(&g, "Changed group scope")).await;
    conn.batch_execute("DROP TRIGGER test_reset_fails ON joppa.state_entries")
        .await?;
    ensure!(
        result.is_err_and(|e| format!("{e:#}").contains("boundary write failed")),
        "the change must be rejected"
    );
    ensure!(
        events(pool.clone(), ws.clone()).await? == count,
        "the journal has a new entry"
    );
    let after = snapshot(&pool, &ws).await?;
    ensure!(after == before, "after: {after}\nbefore: {before}");
    ensure!(
        after["criterion:a"]["status"]["status"] == "green"
            && after["criterion:a"]["assessment"]["state"] == "ready",
        "{after}"
    );
    Ok(())
}

/// Replay the journal into a new projection of the workspace and return its snapshot.
async fn replay(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    pool.get()
        .await?
        .execute("DELETE FROM joppa.state_entries WHERE workspace=$1", &[&ws])
        .await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::migrate(pool).await?;
    snapshot(pool, ws).await
}

/// The number of journal events of the workspace.
async fn events(pool: &deadpool_postgres::Pool, ws: &str) -> Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0))
}

/// A record that passes A and B and fails the check of red criterion B.
async fn upload_full(pool: &deadpool_postgres::Pool, ws: &str, commit: &str) -> Result<Value> {
    let rejects = "tests/auth/login.test.ts > login > rejects";
    store::record_results(
        pool,
        ws,
        "planner",
        &json!({"commit":commit,"results":[{"test":A,"status":"pass"},{"test":B,"status":"pass"},{"test":rejects,"status":"fail","message":"boom"}]}),
    )
    .await
}

fn resave_a() -> Value {
    json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Save the list again"})
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_save_and_a_spec_archive_survive_replay() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    // Red criterion B is a sibling of A under R.
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":"b","revision":0,"parent":"spec:r","data":{"title":"b","owner":"owner"},"reason":"Create the criterion"})).await?;
    write(&pool, &ws, "planner", json!({"op":"set_ac_tests","target":"criterion:b","tests":["tests/auth/login.test.ts > login > rejects"],"reason":"Bind the tests"})).await?;
    upload_full(&pool, &ws, "c2c2c2c").await?;
    let b = status(&pool, &ws, "criterion:b").await?;
    ensure!(b["status"] == "red", "{b}");
    // Fixture 5: the planner saves the identical list of A.
    write(&pool, &ws, "planner", resave_a()).await?;
    let live = snapshot(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "grey"
            && live["criterion:a"]["assessment"]["state"] == "unassessed"
            && live["criterion:d"]["status"]["status"] == "green",
        "{live}"
    );
    ensure!(
        replay(&pool, &ws).await? == live,
        "replay after the list save"
    );
    // A turns green and ready again; fixture 6: the owner archives B.
    let req = read(&pool, &ws, "spec:r").await?["process"]["revision"].clone();
    write(&pool, &ws, "planner", json!({"op":"assess","req":"spec:r","revision":req,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    upload_full(&pool, &ws, "c3c3c3c").await?;
    ensure!(
        status(&pool, &ws, "criterion:a").await?["status"] == "green",
        "A must be green before the archive"
    );
    let b = read(&pool, &ws, "criterion:b").await?["current_revision"].clone();
    write(&pool, &ws, "owner", json!({"op":"archive_object","target":"criterion:b","revision":b,"reason":"Drop the red criterion"})).await?;
    let live = snapshot(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "grey"
            && live["criterion:a"]["assessment"]["state"] == "unassessed"
            && live["criterion:d"]["status"]["status"] == "green",
        "{live}"
    );
    let replayed = replay(&pool, &ws).await?;
    ensure!(replayed == live, "replay: {replayed}\nlive: {live}");
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failed_reset_rejects_the_list_save() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    let before = snapshot(&pool, &ws).await?;
    let count = events(&pool, &ws).await?;
    let conn = pool.get().await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_list_reset_fails() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'list reset failed'; END $$;").await?;
    conn.batch_execute(&format!("CREATE TRIGGER test_list_reset_fails BEFORE INSERT OR UPDATE ON joppa.state_entries FOR EACH ROW WHEN (NEW.workspace='{ws}' AND NEW.section='criterion_boundaries') EXECUTE FUNCTION joppa.test_list_reset_fails()")).await?;
    let result = write(&pool, &ws, "planner", resave_a()).await;
    conn.batch_execute("DROP TRIGGER test_list_reset_fails ON joppa.state_entries")
        .await?;
    ensure!(
        result.is_err_and(|e| format!("{e:#}").contains("list reset failed")),
        "the list save must be rejected"
    );
    ensure!(
        events(&pool, &ws).await? == count,
        "the journal has a new entry"
    );
    let after = snapshot(&pool, &ws).await?;
    ensure!(after == before, "after: {after}\nbefore: {before}");
    ensure!(
        after["criterion:a"]["status"]["status"] == "green"
            && after["criterion:a"]["assessment"]["state"] == "ready",
        "{after}"
    );
    Ok(())
}

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failed_receipt_write_rejects_the_identical_upload() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    change_a(&pool, &ws).await?;
    let (id, receipts) = c1_receipts(&pool, &ws).await?;
    let conn = pool.get().await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_receipt_fails() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'receipt write failed'; END $$;").await?;
    conn.batch_execute(&format!("CREATE TRIGGER test_receipt_fails BEFORE INSERT ON joppa.test_report_receipts FOR EACH ROW WHEN (NEW.report_id={id}) EXECUTE FUNCTION joppa.test_receipt_fails()")).await?;
    let result = upload_c1(&pool, &ws).await;
    conn.batch_execute("DROP TRIGGER test_receipt_fails ON joppa.test_report_receipts")
        .await?;
    ensure!(
        result.is_err_and(|e| format!("{e:#}").contains("receipt write failed")),
        "the upload must fail"
    );
    ensure!(
        c1_receipts(&pool, &ws).await? == (id, receipts),
        "the report has a new receipt"
    );
    let s = status(&pool, &ws, "criterion:a").await?;
    ensure!(s["status"] == "grey", "after the failed upload: {s}");
    Ok(())
}
