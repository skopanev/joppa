// Part 3 of tests/criterion_reset_postgres.rs, included there, so the test names stay unchanged.

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_planning_read_sees_the_receipt_of_an_identical_upload() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    change_a(&pool, &ws).await?;
    // The change cleared the readiness of A; the planner assesses it again.
    let req = read(&pool, &ws, "spec:r").await?["process"]["revision"].clone();
    write(&pool, &ws, "planner", json!({"op":"assess","req":"spec:r","revision":req,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    let claim = |pool: deadpool_postgres::Pool, ws: String| async move {
        let a = read(&pool, &ws, "criterion:a").await?["current_revision"].clone();
        write(
            &pool,
            &ws,
            "planner",
            json!({"op":"claim_planning","ac":"criterion:a","revision":a}),
        )
        .await
    };
    // The planning read gives grey: the claim passes, then the planner releases it.
    claim(pool.clone(), ws.clone()).await?;
    write(
        &pool,
        &ws,
        "planner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Read the status again"}),
    )
    .await?;
    let (id, receipts) = c1_receipts(&pool, &ws).await?;
    let reply = upload_c1(&pool, &ws).await?;
    ensure!(
        reply["record"] == id && reply["replayed"] == true,
        "{reply}"
    );
    ensure!(
        c1_receipts(&pool, &ws).await? == (id, receipts + 1),
        "the repeat must add one receipt"
    );
    let s = status(&pool, &ws, "criterion:a").await?;
    ensure!(s["status"] == "green", "after the record: {s}");
    // The planning read gives green as well: the claim is refused.
    let refused = claim(pool.clone(), ws.clone()).await;
    ensure!(
        refused.is_err_and(|e| format!("{e:#}").contains("criterion is already verified")),
        "a green criterion must refuse planning"
    );
    Ok(())
}
