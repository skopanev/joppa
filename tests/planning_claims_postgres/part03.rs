// Part 3 of tests/planning_claims_postgres.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_block_survives_journal_replay_and_projection_rebuild() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let workspace = planning_workspace(&pool, &["planner", "worker"]).await?;
    save_work(&pool, &workspace, "planner", "t", 0).await?;
    write(
        &pool,
        &workspace,
        "worker",
        json!({"op":"claim_task","id":"work:t","revision":1}),
    )
    .await?;
    let blocked = write(&pool, &workspace, "worker", json!({"op":"block_task","id":"work:t","revision":2,"reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
    let held = read(&pool, &workspace, "work:t").await?;
    assert_eq!(
        held["process"]["blocked"],
        json!({"actor":"worker","reason":"Verification failed","evidence":"test x failed on abc123","position":blocked["position"]})
    );
    assert_eq!(held["process"]["status"], "open");
    assert!(held["process"]["assignee"].is_null());
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    store::migrate(&pool).await?;
    assert_eq!(read(&pool, &workspace, "work:t").await?, held);
    store::check_journal(&pool).await?;
    Ok(())
}
