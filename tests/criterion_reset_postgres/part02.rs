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
        after["criterion:a"]["status"]["status"] == "green",
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

/// The test of red criterion B.
const REJECTS: &str = "tests/auth/login.test.ts > login > rejects";

/// A record that passes A and B and fails the test of red criterion B.
async fn upload_full(pool: &deadpool_postgres::Pool, ws: &str, commit: &str) -> Result<Value> {
    store::record_results(
        pool,
        ws,
        "planner",
        &json!({"commit":commit,"results":[{"test":A,"status":"pass"},{"test":B,"status":"pass"},{"test":REJECTS,"status":"fail","message":"boom"}]}),
    )
    .await
}

/// The test node of `test` directly under `criterion`, as a list row.
async fn test_node(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    criterion: &str,
    test: &str,
) -> Result<Value> {
    let filter = joppa::node_filter::NodeFilter {
        depth: Some(1),
        ..joppa::node_filter::NodeFilter::of(Some(criterion), Some("test"))
    };
    let scope = store::ObjectScope {
        filter,
        limit: 100,
        ..Default::default()
    };
    let tests = store::read_objects(pool, ws, "owner", &scope).await?;
    let b = tests["items"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["title"] == test));
    b.cloned()
        .with_context(|| format!("the test node of {test}: {tests}"))
}

/// The planner gives the test node of B under criterion A a new body. A new title alone
/// resets nothing, so the change is a new data value.
async fn change_b_of_a(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    let b = test_node(pool, ws, "criterion:a", B).await?;
    let address = b["address"].as_str().context("address")?;
    let (_, id) = address.split_once(':').context("address")?;
    write(pool, ws, "planner", json!({"op":"save_object","kind":"test","id":id,"revision":b["revision"],"parent":"criterion:a","data":{"title":B,"owner":"owner","body":"Refuse a second sign-in"},"reason":"Change the test"})).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_test_node_change_and_a_criterion_archive_survive_replay() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    // Red criterion B is a sibling of A under R.
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":"b","revision":0,"parent":"spec:r","data":{"title":"b","owner":"owner"},"reason":"Create the criterion"})).await?;
    write(&pool, &ws, "planner", bind("criterion:b", REJECTS)).await?;
    upload_full(&pool, &ws, "c2c2c2c").await?;
    let b = status(&pool, &ws, "criterion:b").await?;
    ensure!(b["status"] == "red", "{b}");
    // Fixture 5: the planner changes a test node of A. It passed on C2, before the change,
    // so that pass no longer counts.
    let changed = change_b_of_a(&pool, &ws).await?;
    let node = changed["object"]["address"]
        .as_str()
        .context("the changed node")?;
    let node = status(&pool, &ws, node).await?;
    ensure!(node["status"] == "grey", "the changed node: {node}");
    let live = snapshot(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "grey"
            && live["criterion:d"]["status"]["status"] == "green",
        "{live}"
    );
    ensure!(
        replay(&pool, &ws).await? == live,
        "replay after the test-node change"
    );
    // A turns green again. Fixture 6: the owner archives B.
    upload_full(&pool, &ws, "c3c3c3c").await?;
    ensure!(
        status(&pool, &ws, "criterion:a").await?["status"] == "green",
        "A must be green before the archive"
    );
    // B has an active test node, so the node is archived first.
    let node = test_node(&pool, &ws, "criterion:b", REJECTS).await?;
    write(&pool, &ws, "owner", json!({"op":"archive_object","target":node["address"],"revision":node["revision"],"reason":"Drop the red criterion"})).await?;
    let b = read(&pool, &ws, "criterion:b").await?["current_revision"].clone();
    write(&pool, &ws, "owner", json!({"op":"archive_object","target":"criterion:b","revision":b,"reason":"Drop the red criterion"})).await?;
    // An archive resets only the criteria at and below the archived object: A stays.
    let live = snapshot(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "green"
            && live["criterion:d"]["status"]["status"] == "green",
        "{live}"
    );
    let replayed = replay(&pool, &ws).await?;
    ensure!(replayed == live, "replay: {replayed}\nlive: {live}");
    // An archive before the epoch of archive_own_reset keeps its old meaning in a replay:
    // it resets the criteria at and below the parent, so A is grey.
    let after = events(&pool, &ws).await? + 1;
    pool.get().await?.execute("UPDATE joppa.replay_epochs SET first_position=$2 WHERE workspace=$1 AND rule='archive_own_reset'", &[&ws, &after]).await?;
    let old = replay(&pool, &ws).await?;
    ensure!(
        old["criterion:a"]["status"]["status"] == "grey"
            && old["criterion:d"]["status"]["status"] == "green",
        "{old}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failed_reset_rejects_the_test_node_change() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    let before = snapshot(&pool, &ws).await?;
    let count = events(&pool, &ws).await?;
    let conn = pool.get().await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_list_reset_fails() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'list reset failed'; END $$;").await?;
    conn.batch_execute(&format!("CREATE TRIGGER test_list_reset_fails BEFORE INSERT OR UPDATE ON joppa.state_entries FOR EACH ROW WHEN (NEW.workspace='{ws}' AND NEW.section='criterion_boundaries') EXECUTE FUNCTION joppa.test_list_reset_fails()")).await?;
    let result = change_b_of_a(&pool, &ws).await;
    conn.batch_execute("DROP TRIGGER test_list_reset_fails ON joppa.state_entries")
        .await?;
    ensure!(
        result.is_err_and(|e| format!("{e:#}").contains("list reset failed")),
        "the test-node change must be rejected"
    );
    ensure!(
        events(&pool, &ws).await? == count,
        "the journal has a new entry"
    );
    let after = snapshot(&pool, &ws).await?;
    ensure!(after == before, "after: {after}\nbefore: {before}");
    ensure!(
        after["criterion:a"]["status"]["status"] == "green",
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
