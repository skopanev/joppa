//! The test rules come only from the schema: a change of a rule in the block test_results
//! changes what Joppa accepts and how it colours a criterion.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

const C1: &str = "c0ffee7";

async fn pool() -> Result<deadpool_postgres::Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok(pool)
}

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<Value> {
    store::execute_remote_input(pool, ws, "bot", &uuid::Uuid::new_v4().to_string(), &command).await
}

/// Requirement R with criterion A. `rules` changes keys of the block test_results.
async fn fixture(pool: &deadpool_postgres::Pool, rules: Value) -> Result<String> {
    let ws = format!("rules-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Test rules"}),
    )
    .await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    let mut block = tree["schema"]["test_results"].clone();
    for (key, value) in rules.as_object().cloned().unwrap_or_default() {
        block[key] = value;
    }
    write(pool, &ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","root":true,"parents":[],"status":"children"},
        "ac":{"title":"Criteria","role":"ac","parents":["requirement"],"status":"children"},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400}
    },"test_results":block})).await?;
    for command in [
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"}),
    ] {
        write(pool, &ws, command).await?;
    }
    Ok(ws)
}

/// Save one test node under A for each test name, in order. The first refusal stops.
async fn bind(pool: &deadpool_postgres::Pool, ws: &str, tests: Value) -> Result<()> {
    for test in tests.as_array().cloned().unwrap_or_default() {
        write(pool, ws, json!({"op":"save_object","kind":"test","revision":0,"parent":"ac:a","data":{"title":test,"owner":"owner"},"reason":"Bind"})).await?;
    }
    Ok(())
}

async fn record(pool: &deadpool_postgres::Pool, ws: &str, results: Value) -> Result<()> {
    store::record_results(pool, ws, "bot", &json!({"commit":C1,"results":results})).await?;
    Ok(())
}

async fn status(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    Ok(store::test_status(pool, ws, "bot", "ac:a").await?["status"].clone())
}

// test-rules-in-schema-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_green_and_same_commit_rules_come_from_the_schema() -> Result<()> {
    let pool = pool().await?;
    const X: &str = "tests/x.rs > x";
    // With skip in green_if_all, a skip makes the criterion green.
    let ws = fixture(&pool, json!({"green_if_all":["pass","skip"]})).await?;
    bind(&pool, &ws, json!([X])).await?;
    record(&pool, &ws, json!([{"test":X,"status":"skip"}])).await?;
    ensure!(status(&pool, &ws).await? == "green", "skip in green_if_all");
    // With latest_wins, a later pass on the same commit beats an earlier failure.
    for (rule, expected) in [("fail_wins", "red"), ("latest_wins", "green")] {
        let ws = fixture(&pool, json!({"same_commit":rule})).await?;
        bind(&pool, &ws, json!([X])).await?;
        record(&pool, &ws, json!([{"test":X,"status":"fail"}])).await?;
        record(&pool, &ws, json!([{"test":X,"status":"pass"}])).await?;
        ensure!(status(&pool, &ws).await? == expected, "{rule}");
    }
    Ok(())
}

// test-rules-in-schema-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_reset_rule_comes_from_the_schema() -> Result<()> {
    let pool = pool().await?;
    const X: &str = "tests/x.rs > x";
    for (reset, expected) in [(true, "grey"), (false, "green")] {
        let ws = fixture(&pool, json!({"reset_on_change":reset})).await?;
        bind(&pool, &ws, json!([X])).await?;
        record(&pool, &ws, json!([{"test":X,"status":"pass"}])).await?;
        ensure!(status(&pool, &ws).await? == "green", "before the change");
        write(&pool, &ws, json!({"op":"save_object","kind":"ac","id":"a","revision":1,"parent":"requirement:r","data":{"title":"A changed","owner":"owner"},"reason":"New wording"})).await?;
        ensure!(status(&pool, &ws).await? == expected, "reset {reset}");
    }
    Ok(())
}

// test-rules-in-schema-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_limit_rules_come_from_the_schema() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool, json!({"max_tests":1,"max_results":1})).await?;
    // No name rule checks a title: a business title saves.
    bind(&pool, &ws, json!(["A refund over the limit is refused"])).await?;
    // The one test node of A fills the limit, so a second node is over it.
    let message = match bind(&pool, &ws, json!(["a::b"])).await {
        Ok(()) => anyhow::bail!("saved a second test node, expected the limit"),
        Err(e) => e.to_string(),
    };
    ensure!(
        message.contains("a criterion lists at most 1 tests"),
        "{message}"
    );
    let error = store::record_results(
        &pool,
        &ws,
        "bot",
        &json!({"commit":C1,"results":[
        {"test":"a::b","status":"pass"},{"test":"c::d","status":"pass"}]}),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(error.contains("a record has at most 1 results"), "{error}");
    Ok(())
}

// tests-in-document
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_test_node_save_keeps_the_criterion_revision_and_a_text_change_moves_it() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool, json!({})).await?;
    let read = || async {
        store::read_objects(
            &pool,
            &ws,
            "bot",
            &store::ObjectScope {
                address: Some("ac:a"),
                ..Default::default()
            },
        )
        .await
    };
    let before = read().await?;
    ensure!(
        before["current_revision"] == 1 && before.get("contract_revision").is_none(),
        "{before}"
    );
    bind(&pool, &ws, json!(["tests/x.rs > x"])).await?;
    let bound = read().await?;
    ensure!(
        bound["current_revision"] == 1,
        "a test node save keeps the criterion revision: {bound}"
    );
    // A list row of the direct children gives the same revision and no contract revision.
    let rows = store::read_objects(
        &pool,
        &ws,
        "bot",
        &store::ObjectScope {
            filter: joppa::node_filter::NodeFilter {
                depth: Some(1),
                ..joppa::node_filter::NodeFilter::of(Some("requirement:r"), None)
            },
            limit: 100,
            ..Default::default()
        },
    )
    .await?;
    let row = &rows["items"][0];
    ensure!(
        row["address"] == "ac:a" && row["revision"] == 1 && row.get("contract_revision").is_none(),
        "the list row: {row}"
    );
    write(&pool, &ws, json!({"op":"save_object","kind":"ac","id":"a","revision":1,"parent":"requirement:r","data":{"title":"A changed","owner":"owner"},"reason":"New wording"})).await?;
    let changed = read().await?;
    ensure!(
        changed["current_revision"] == 2,
        "a text change makes a new revision: {changed}"
    );
    Ok(())
}
