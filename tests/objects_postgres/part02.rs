// Part 2 of tests/objects_postgres.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn note_queue_filters_links_and_color_preserve_board_structure() -> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let ws = format!(
        "note-queue-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["note"]["color"] = json!("#7655a2");
    types["note"]["title_size"] = json!(21);
    types["note"]["title"] = json!("Incoming note");
    types["domain"]["index"] = json!(200);
    store::execute_remote_input(&pool, &ws, "owner", "color", &json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Distinguish source Notes"})).await?;
    let board = store::board_read(&pool, &ws, "agent").await?;
    assert_eq!(board["custom_tree"], false);
    assert_eq!(board["object_types"]["note"]["color"], "#7655a2");
    assert_eq!(board["object_types"]["note"]["title_size"], 21);
    assert_eq!(board["object_types"]["domain"]["index"], 200);
    note_queue_links_and_color_after_setup(&pool, &ws).await
}
