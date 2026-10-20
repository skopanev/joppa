//! The status list of the tree schema: bad lists are refused, a later save keeps the list,
//! and the list order decides the status of a children rule.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

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

fn types() -> Value {
    json!({
        "domain":{"title":"Domains","role":"group","root":true,"parents":[]},
        "requirement":{"title":"Requirements","role":"requirement","parents":["domain"]},
        "ac":{"title":"Criteria","role":"ac","parents":["requirement"]}
    })
}

async fn workspace(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("statuses-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Status list"}),
    )
    .await?;
    write(
        pool,
        &ws,
        "owner",
        json!({"op":"enroll_service","id":"bot","role":"agent"}),
    )
    .await?;
    Ok(ws)
}

async fn tree(pool: &deadpool_postgres::Pool, ws: &str, revision: Option<u32>) -> Result<Value> {
    store::tree_schema(pool, ws, "owner", revision).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_unknown_repeated_or_badly_coloured_tone_is_refused() -> Result<()> {
    // ac:4d77e8b4: three bad lists on schema revision 0.
    let pool = pool().await?;
    let ws = workspace(&pool).await?;
    let before = tree(&pool, &ws, None).await?;
    let revision = before["schema"]["revision"].clone();
    for statuses in [
        json!([{"tone":"blue","light":"#000000","dark":"#000000"}]),
        json!([{"tone":"red","light":"#000000","dark":"#000000"},{"tone":"red","light":"#111111","dark":"#111111"}]),
        json!([{"tone":"red","light":"#000000","dark":"red"}]),
    ] {
        write(&pool, &ws, "owner", json!({"op":"set_tree_schema","revision":revision,"types":types(),"statuses":statuses,"reason":"Bad list"}))
            .await
            .expect_err("a bad status list must be refused");
    }
    let after = tree(&pool, &ws, None).await?;
    ensure!(after["schema"]["revision"] == revision, "{after}");
    let defaults = serde_json::to_value(joppa::status_colors::defaults())?;
    ensure!(after["schema"]["statuses"] == defaults, "{after}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_ordered_list_saves_and_a_higher_tone_wins_in_a_children_rule() -> Result<()> {
    // ac:71a034aa: green, red and grey in this order; revision 2 has no statuses.
    let pool = pool().await?;
    let ws = workspace(&pool).await?;
    let start = tree(&pool, &ws, None).await?["schema"]["revision"].clone();
    let statuses = json!([
        {"tone":"green","light":"#22a559","dark":"#2fbf6a"},
        {"tone":"red","light":"#bc5d52","dark":"#e07a70"},
        {"tone":"grey","light":"#c8c3b8","dark":"#8a8f96"}
    ]);
    write(&pool, &ws, "owner", json!({"op":"set_tree_schema","revision":start,"types":types(),"statuses":statuses,"reason":"Order the statuses"})).await?;
    let first = tree(&pool, &ws, None).await?;
    let one = first["schema"]["revision"].as_u64().unwrap_or_default() as u32;
    write(
        &pool,
        &ws,
        "owner",
        json!({"op":"set_tree_schema","revision":one,"types":types(),"reason":"Other change"}),
    )
    .await?;
    let second = tree(&pool, &ws, None).await?;
    ensure!(second["schema"]["revision"] == one + 1, "{second}");
    let saved = tree(&pool, &ws, Some(one)).await?;
    ensure!(
        saved["schema"]["statuses"]
            .as_array()
            .map(|l| l[..3].to_vec())
            == statuses.as_array().cloned(),
        "{saved}"
    );
    ensure!(
        second["schema"]["statuses"] == saved["schema"]["statuses"],
        "{second}"
    );

    // Requirement R has criterion A with a pass and criterion B with a fail.
    for command in [
        json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"Domain","owner":"owner"},"reason":"Create"}),
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":"domain:d","data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r","data":{"title":"B","owner":"owner"},"reason":"Create"}),
        json!({"op":"set_ac_tests","target":"ac:a","tests":["tests/x.rs > x"],"reason":"Bind"}),
        json!({"op":"set_ac_tests","target":"ac:b","tests":["tests/y.rs > y"],"reason":"Bind"}),
    ] {
        write(&pool, &ws, "bot", command).await?;
    }
    store::record_results(
        &pool,
        &ws,
        "bot",
        &json!({"commit":"c0ffee1","results":[
            {"test":"tests/x.rs > x","status":"pass"},{"test":"tests/y.rs > y","status":"fail"}
        ]}),
    )
    .await?;
    for (ac, tone) in [("ac:a", "green"), ("ac:b", "red")] {
        let status = store::criterion_tests(&pool, &ws, "bot", ac).await?;
        ensure!(status["status"] == tone, "{ac}: {status}");
    }
    let r = store::read_objects(
        &pool,
        &ws,
        "bot",
        &store::ObjectScope {
            address: Some("requirement:r"),
            ..Default::default()
        },
    )
    .await?;
    ensure!(r["signal"]["tone"] == "green", "green is above red: {r}");
    Ok(())
}
