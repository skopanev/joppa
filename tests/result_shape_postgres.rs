//! A workspace sets the shape of a test result in its tree schema. record_results checks
//! each result against it, and joppa_tests shows the status name and the fields.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

const X: &str = "tests/shape.rs > x";
const Y: &str = "tests/shape.rs > y";
const C: &str = "c0ffee3";

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

/// One workspace: criterion A lists X and Y, and bot is a member.
async fn fixture(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("shape-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Result shape"}),
    )
    .await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    write(pool, &ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]}
    }})).await?;
    for command in [
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"}),
        json!({"op":"set_ac_tests","target":"ac:a","tests":[X,Y],"reason":"Bind"}),
    ] {
        write(pool, &ws, command).await?;
    }
    Ok(ws)
}

async fn set_shape(pool: &deadpool_postgres::Pool, ws: &str, shape: Value) -> Result<Value> {
    let tree = store::tree_schema(pool, ws, "bot", None).await?;
    // The command sends the whole block: the current rules with the changed keys.
    let mut block = tree["schema"]["test_results"].clone();
    for (key, value) in shape.as_object().cloned().unwrap_or_default() {
        if key == "fields" {
            let mut fields = block["fields"].as_array().cloned().unwrap_or_default();
            fields.extend(value.as_array().cloned().unwrap_or_default());
            block["fields"] = json!(fields);
        } else {
            block[key] = value;
        }
    }
    write(pool, ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Set the result shape","test_results":block})).await
}

async fn record(pool: &deadpool_postgres::Pool, ws: &str, results: Value) -> Result<Value> {
    store::record_results(pool, ws, "bot", &json!({"commit":C,"results":results})).await
}

fn browser_shape() -> Value {
    json!({
        "statuses": {"passed": "pass", "broken": "fail"},
        "fields": [
            {"name": "browser", "type": "enum", "values": ["chromium", "webkit"], "required": true},
            {"name": "run_url", "title": "Run", "type": "text", "max_length": 200}
        ]
    })
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_workspace_without_a_shape_keeps_the_built_in_result() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool).await?;
    // The schema of a new workspace holds the older rules as data.
    let tree = store::tree_schema(&pool, &ws, "bot", None).await?;
    ensure!(
        tree["schema"]["test_results"] == serde_json::to_value(joppa::test_rules::legacy())?,
        "the rules in the schema: {tree}"
    );
    record(
        &pool,
        &ws,
        json!([{"test":X,"status":"pass"},{"test":Y,"status":"pass"}]),
    )
    .await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(s["status"] == "green", "{s}");
    let test = &s["tests"][0];
    ensure!(
        test["status_name"] == "pass" && test.get("fields").is_none(),
        "{test}"
    );
    let error = record(
        &pool,
        &ws,
        json!([{"test":X,"status":"pass","browser":"webkit"}]),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(error.contains("unknown result field `browser`"), "{error}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_saved_shape_decides_the_status_names_and_the_fields() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool).await?;
    set_shape(&pool, &ws, browser_shape()).await?;
    let tree = store::tree_schema(&pool, &ws, "bot", None).await?;
    let block = &tree["schema"]["test_results"];
    ensure!(
        block["statuses"] == browser_shape()["statuses"]
            && block["fields"].as_array().is_some_and(|f| f.len() == 4)
            && block["failure_field"] == "message",
        "saved shape: {tree}"
    );
    ensure!(
        tree["schema"]["types"].get("ac").is_some(),
        "the types stay: {tree}"
    );
    for (results, error) in [
        (
            json!([{"test":X,"status":"pass","browser":"webkit"}]),
            "status must be one of: broken, passed",
        ),
        (
            json!([{"test":X,"status":"passed"}]),
            "result field browser is required",
        ),
        (
            json!([{"test":X,"status":"passed","browser":"firefox"}]),
            "needs one of: chromium, webkit",
        ),
    ] {
        let message = record(&pool, &ws, results.clone())
            .await
            .unwrap_err()
            .to_string();
        ensure!(message.contains(error), "{results}: {message}");
    }
    record(
        &pool,
        &ws,
        json!([
            {"test":X,"status":"passed","browser":"chromium","run_url":"https://ci.example/1"},
            {"test":Y,"status":"passed","browser":"webkit"}
        ]),
    )
    .await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(s["status"] == "green", "{s}");
    ensure!(
        s["tests"][0]["status"] == "pass"
            && s["tests"][0]["status_name"] == "passed"
            && s["tests"][0]["fields"]
                == json!({"browser":"chromium","run_url":"https://ci.example/1"}),
        "{s}"
    );
    record(
        &pool,
        &ws,
        json!([{"test":Y,"status":"broken","browser":"webkit","message":"timeout"}]),
    )
    .await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(s["status"] == "red", "{s}");
    let y = &s["tests"][1];
    ensure!(
        y["status"] == "fail" && y["status_name"] == "broken" && y["message"] == "timeout",
        "{y}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_schema_change_without_a_shape_keeps_it_and_a_bad_shape_is_refused() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool).await?;
    set_shape(&pool, &ws, browser_shape()).await?;
    let tree = store::tree_schema(&pool, &ws, "bot", None).await?;
    write(&pool, &ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"New priorities","priorities":["now","later"]})).await?;
    let tree = store::tree_schema(&pool, &ws, "bot", None).await?;
    ensure!(
        tree["schema"]["test_results"]["statuses"] == browser_shape()["statuses"],
        "kept shape: {tree}"
    );
    let error = set_shape(&pool, &ws, json!({"statuses": {"ok": "pass"}}))
        .await
        .unwrap_err()
        .to_string();
    ensure!(
        error.contains("needs a name that counts as fail"),
        "{error}"
    );
    let error = set_shape(
        &pool,
        &ws,
        json!({"fields": [{"name": "commit", "type": "text", "max_length": 10}]}),
    )
    .await
    .unwrap_err()
    .to_string();
    ensure!(
        error.contains("result field commit is part of the record"),
        "{error}"
    );
    // A block without every rule is refused.
    let tree = store::tree_schema(&pool, &ws, "bot", None).await?;
    let error = write(&pool, &ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Part of the rules","test_results":{"statuses":{"ok":"pass","bad":"fail"}}}))
        .await
        .unwrap_err()
        .to_string();
    ensure!(
        error.contains("test_results needs every rule; missing: fields, name"),
        "{error}"
    );
    Ok(())
}
