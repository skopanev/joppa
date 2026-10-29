// Part 2 of tests/board_field_postgres.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_result_without_the_board_field_counts_for_every_value() -> Result<()> {
    let pool = pool().await?;
    let shape = json!({"fields":[{"name":"app","title":"App","type":"enum","values":["ios","android"],"board":true}]});
    let ws = fixture(&pool, Some(shape)).await?;
    // A shared test without app counts for both values.
    record(&pool, &ws, C1, json!([{"test":X,"status":"pass"}])).await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(
        s["status"] == "green"
            && s["values"]["app"]["ios"]["status"] == "green"
            && s["values"]["app"]["android"]["status"] == "green",
        "{s}"
    );
    // A later failure for android only makes android red.
    record(
        &pool,
        &ws,
        C2,
        json!([{"test":X,"status":"fail","app":"android"}]),
    )
    .await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(
        s["status"] == "red"
            && s["values"]["app"]["ios"]["status"] == "green"
            && s["values"]["app"]["android"]["status"] == "red",
        "{s}"
    );
    Ok(())
}
