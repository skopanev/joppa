// Part 2 of tests/asset_board_postgres.rs, included there, so the test names stay unchanged.

// asset-board-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_names_its_assets_or_the_explicit_value_all() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool).await?;
    let error = write(
        &pool,
        &ws,
        save(
            "ac",
            "c",
            Some("requirement:r"),
            json!({"title":"C","owner":"owner","tests":[X]}),
        ),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(
        error.contains("reference applies_to is required"),
        "{error}"
    );
    let error = write(
        &pool,
        &ws,
        save(
            "ac",
            "c",
            Some("requirement:r"),
            json!({"title":"C","owner":"owner","applies_to":[],"tests":[X]}),
        ),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(
        error.contains("reference applies_to is required"),
        "{error}"
    );
    let error = write(
        &pool,
        &ws,
        save(
            "ac",
            "c",
            Some("requirement:r"),
            json!({"title":"C","owner":"owner","applies_to":["web"],"tests":[X]}),
        ),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(error.contains("unknown reference target"), "{error}");
    Ok(())
}
