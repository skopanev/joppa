// Part 4 of tests/readiness_metadata.rs, included there, so the test names stay unchanged.

// A new workspace stores an epoch at position 1 for each replay rule. A journal without the
// epoch of a rule refuses to replay: the replay cannot tell the old meaning of its events.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_journal_without_a_replay_epoch_refuses_to_replay() -> anyhow::Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let conn = pool.get().await?;
    let name: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("epochs-{}", uuid::Uuid::new_v4().simple());
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    let epochs: Vec<(String, i64)> = conn
        .query(
            "SELECT rule,first_position FROM joppa.replay_epochs WHERE workspace=$1 ORDER BY rule",
            &[&ws],
        )
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    let mut expected: Vec<(String, i64)> = joppa::commands::REPLAY_RULES
        .iter()
        .map(|rule| (rule.to_string(), 1))
        .collect();
    expected.sort();
    assert_eq!(epochs, expected);
    store::check_journal(&pool).await?;
    conn.execute(
        "DELETE FROM joppa.replay_epochs WHERE workspace=$1 AND rule=$2",
        &[&ws, &joppa::commands::LIST_SAVE_RESET],
    )
    .await?;
    let error = format!("{:#}", store::check_journal(&pool).await.unwrap_err());
    // Restore the epoch first, so that later tests in this database can replay every journal.
    conn.execute(
        "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) VALUES($1,$2,1)",
        &[&ws, &joppa::commands::LIST_SAVE_RESET],
    )
    .await?;
    assert!(
        error.contains(&format!(
            "workspace {ws} has no replay epoch for rule list_save_reset"
        )),
        "{error}"
    );
    store::check_journal(&pool).await?;
    Ok(())
}
