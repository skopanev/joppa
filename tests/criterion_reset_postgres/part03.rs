// Part 3 of tests/criterion_reset_postgres.rs, included there, so the test names stay unchanged.

/// The snapshot with the readiness mark of criterion C.
async fn marks(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    let mut marks = snapshot(pool, ws).await?;
    marks["criterion:c"] = read(pool, ws, "criterion:c").await?["process"]["assessment"].clone();
    Ok(marks)
}

// ac:kp5dhbh7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_parent_change_keeps_the_readiness_marks_below_it() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    // Criterion C under R waits for an answer.
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":"c","revision":0,"parent":"spec:r","data":{"title":"c","owner":"owner"},"reason":"Create the criterion"})).await?;
    write(&pool, &ws, "planner", json!({"op":"assess","target":"criterion:c","ready":false,"reason":"Which limit applies?"})).await?;
    // The owner changes the text of R: the tests below stop counting, the marks stay.
    let r = read(&pool, &ws, "spec:r").await?["current_revision"].clone();
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"spec","id":"r","revision":r,"parent":"area:g","data":{"title":"r","body":"Implement the sharper rule","owner":"owner"},"reason":"Sharper rule"})).await?;
    let live = marks(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "grey"
            && live["criterion:a"]["assessment"]["state"] == "ready"
            && live["criterion:c"]["state"] == "waiting"
            && live["criterion:c"]["reason"] == "Which limit applies?"
            && live["criterion:d"]["assessment"]["state"] == "ready",
        "after the spec change: {live}"
    );
    // The owner changes the text of A: only A loses its mark.
    change_a(&pool, &ws).await?;
    let live = marks(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["assessment"].is_null()
            && live["criterion:c"]["state"] == "waiting"
            && live["criterion:d"]["assessment"]["state"] == "ready",
        "after the criterion change: {live}"
    );
    replay(&pool, &ws).await?;
    let replayed = marks(&pool, &ws).await?;
    ensure!(replayed == live, "replay: {replayed}\nlive: {live}");
    // A change before the epoch of own_mark_reset keeps its old meaning in a replay: the
    // change of R removes every mark below R.
    let after = events(&pool, &ws).await? + 1;
    pool.get().await?.execute("UPDATE joppa.replay_epochs SET first_position=$2 WHERE workspace=$1 AND rule='own_mark_reset'", &[&ws, &after]).await?;
    replay(&pool, &ws).await?;
    let old = marks(&pool, &ws).await?;
    ensure!(
        old["criterion:a"]["assessment"].is_null()
            && old["criterion:c"].is_null()
            && old["criterion:d"]["assessment"]["state"] == "ready",
        "{old}"
    );
    Ok(())
}
