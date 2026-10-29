// Part 2 of tests/search.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn current_search_is_scoped_fresh_rebuildable_and_hybrid_without_touching_history()
-> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let ws = format!("search-{}", uuid::Uuid::new_v4().simple());
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "extra",
        &json!({"op":"save_object","kind":"domain","id":"unused","revision":0,"parent":null,"data":{"title":"Invoice handling","owner":"owner"},"reason":"Setup"}),
    )
    .await?;
    let current = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("requirement:r"),
            ..Default::default()
        },
    )
    .await?;
    let mut data = current["document"]["data"].clone();
    data["title"] = json!("Subscription renewal preserves access");
    data["body"] = json!("Платежи за подписку принимаются на iOS.");
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "wording",
        &json!({"op":"save_object","kind":"requirement","id":"r",
        "revision":current["current_revision"],"parent":current["document"]["parent"],"data":data,
        "links":current["document"]["links"],"reason":"Agreed wording"}),
    )
    .await?;
    let before = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    let text = store::search::find(
        &pool,
        "agent",
        &ws,
        "renewal",
        Some("requirement"),
        10,
        None,
    )
    .await?;
    assert_eq!(text["mode"], "full_text");
    assert_eq!(text["semantic_status"], "not_configured");
    assert_eq!(text["items"][0]["id"], "r");
    assert_eq!(text["items"][0]["revision"], 2);
    let russian = store::search::find(
        &pool,
        "agent",
        &ws,
        "платежа",
        Some("requirement"),
        10,
        None,
    )
    .await?;
    assert_eq!(russian["items"][0]["id"], "r");
    let public = before["system_ids"]["ac"]["ios"].as_str().unwrap();
    assert_eq!(
        store::search::find(&pool, "agent", &ws, public, None, 10, None).await?["items"][0]["system_id"],
        public
    );
    assert!(
        store::search::find(&pool, "outsider", &ws, "renewal", None, 10, None)
            .await
            .is_err()
    );
    let foreign = format!("foreign-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &foreign,
        "other",
        "create",
        &json!({"op":"workspace","title":"Private"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &foreign,
        "other",
        "domain",
        &json!({"op":"save_object","kind":"domain","id":"secret","revision":0,"parent":null,"data":{"title":"Foreign-only needle","owner":"other"},"reason":"Setup"}),
    )
    .await?;
    assert!(
        store::search::find(&pool, "owner", &ws, "foreign-only", None, 10, None).await?["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A missing cache is reconstructed from the unchanged journal on an authorized search.
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.search_documents WHERE workspace=$1",
            &[&ws],
        )
        .await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.search_state WHERE workspace=$1", &[&ws])
        .await?;
    assert_eq!(
        store::search::find(
            &pool,
            "agent",
            &ws,
            "renewal",
            Some("requirement"),
            10,
            None
        )
        .await?["items"],
        text["items"]
    );
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
        before
    );
    hybrid_search_after_the_index_reads(&pool, &ws, public).await
}
