//! A criterion is green only when every listed test has a pass recorded after the last
//! change of the criterion. Records of one commit in two calls combine.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

const X: &str = "tests/green.rs > x";
const Y: &str = "tests/green.rs > y";
const C: &str = "c0ffee2";

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

async fn write(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    command: Value,
) -> Result<Value> {
    // An agent or a runner enrolls through the internal event, as an agent token does.
    if command["op"] == "enroll_service" {
        return store::enroll_service(
            pool,
            ws,
            actor,
            command["id"].as_str().unwrap_or_default(),
            command["role"].as_str().unwrap_or_default(),
        )
        .await;
    }
    store::execute_remote_input(pool, ws, actor, &uuid::Uuid::new_v4().to_string(), &command).await
}

/// One independent fixture: criterion A lists X and Y, and bot is a member.
async fn fixture(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("green-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Recorded green"}),
    )
    .await?;
    write(
        pool,
        &ws,
        "owner",
        json!({"op":"enroll_service","id":"bot","role":"agent"}),
    )
    .await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    write(pool, &ws, "owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]}
    }})).await?;
    for command in [
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"}),
        json!({"op":"set_ac_tests","target":"ac:a","tests":[X,Y],"reason":"Bind"}),
    ] {
        write(pool, &ws, "bot", command).await?;
    }
    Ok(ws)
}

async fn record(pool: &deadpool_postgres::Pool, ws: &str, tests: &[&str]) -> Result<()> {
    let results: Vec<Value> = tests
        .iter()
        .map(|t| json!({"test":t,"status":"pass"}))
        .collect();
    store::record_results(pool, ws, "bot", &json!({"commit":C,"results":results})).await?;
    Ok(())
}

async fn status(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    store::criterion_tests(pool, ws, "bot", "ac:a").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_record_after_the_last_change_makes_a_criterion_green() -> Result<()> {
    // ac:66f0d4c3, fixture 1: X and Y pass for commit C in two calls.
    let pool = pool().await?;
    let one = fixture(&pool).await?;
    record(&pool, &one, &[X]).await?;
    record(&pool, &one, &[Y]).await?;
    let s = status(&pool, &one).await?;
    ensure!(s["status"] == "green", "fixture 1: {s}");

    // Fixture 2: only X has a pass.
    let two = fixture(&pool).await?;
    record(&pool, &two, &[X]).await?;
    let s = status(&pool, &two).await?;
    ensure!(s["status"] == "grey", "fixture 2: {s}");

    // Fixture 3: X and Y pass, then a member changes the text of A.
    let three = fixture(&pool).await?;
    record(&pool, &three, &[X, Y]).await?;
    ensure!(
        status(&pool, &three).await?["status"] == "green",
        "fixture 3 before the change"
    );
    write(&pool, &three, "bot", json!({"op":"save_object","kind":"ac","id":"a","revision":2,"parent":"requirement:r","data":{"title":"A changed","owner":"owner","tests":[X,Y]},"reason":"Sharper wording"})).await?;
    let s = status(&pool, &three).await?;
    ensure!(s["status"] == "grey", "fixture 3: {s}");
    Ok(())
}
