//! Ready for work is the only step before work starts. A tree schema without process
//! fields gives a requirement no confirmation and no acceptance.
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
    store::execute_remote_input(pool, ws, actor, &uuid::Uuid::new_v4().to_string(), &command).await
}

async fn read(pool: &deadpool_postgres::Pool, ws: &str, address: &str) -> Result<Value> {
    store::read_objects(
        pool,
        ws,
        "alex",
        &store::ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_writes_a_requirement_and_work_starts_after_ready() -> Result<()> {
    // ac:ready-is-the-only-gate-ac1: alex has write access and is not the owner.
    let pool = pool().await?;
    let ws = format!("ready-gate-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Ready gate"}),
    )
    .await?;
    write(
        &pool,
        &ws,
        "owner",
        json!({"op":"member","id":"alex","role":"human"}),
    )
    .await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    write(&pool, &ws, "owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"A tree without process fields","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]},
        "task":{"title":"Tasks","role":"task","status":"none","parents":["ac"],"parent_gate":"ready"}
    }})).await?;
    // R and its criterion A are one change: one batch.
    let operations: Vec<store::BatchOperation> = [
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"alex","body":"The rule"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"alex"},"reason":"Create"}),
    ]
    .into_iter()
    .map(|command| store::BatchOperation { reference: None, command })
    .collect();
    store::execute_remote_batch(&pool, &ws, "alex", "create-r", &operations).await?;
    let r = read(&pool, &ws, "requirement:r").await?;
    write(&pool, &ws, "alex", json!({"op":"assess","req":"requirement:r","revision":r["process"]["revision"],"ac":"ac:a","assessment":{"ready":true,"reason":"Clear"}})).await?;
    write(&pool, &ws, "alex", json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"ac:a","data":{"title":"T","owner":"alex","contract":"Implement A","purpose":"implementation"},"reason":"Create work"})).await?;
    let t = read(&pool, &ws, "task:t").await?;
    write(
        &pool,
        &ws,
        "alex",
        json!({"op":"claim_task","id":"task:t","revision":t["current_revision"]}),
    )
    .await?;

    let a = read(&pool, &ws, "ac:a").await?;
    ensure!(a["process"]["assessment"]["state"] == "ready", "{a}");
    let timeline = store::read_timeline(
        &pool,
        &ws,
        "alex",
        &store::TimelineScope {
            after: 0,
            limit: 100,
            cursor: None,
            order: None,
        },
    )
    .await?;
    let readiness = timeline["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["op"] == "assess");
    ensure!(
        readiness.is_some_and(|item| item["actor"] == "alex"),
        "{timeline}"
    );
    // R has two revisions: its own save and the save of A under it. Only alex acts in the
    // chain after the owner set up the workspace.
    let r = read(&pool, &ws, "requirement:r").await?;
    ensure!(
        r["current_revision"] == 2 && r["document"]["actor"] == "alex",
        "{r}"
    );
    let actors: Vec<Value> = timeline["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            item["op"] != "workspace" && item["op"] != "member" && item["op"] != "set_tree_schema"
        })
        .filter(|item| {
            !item["op"]
                .as_str()
                .is_some_and(|op| op.starts_with("migrate"))
        })
        .map(|item| item["actor"].clone())
        .collect();
    ensure!(
        !actors.is_empty() && actors.iter().all(|actor| actor == "alex"),
        "{actors:?}"
    );
    let process = r["process"].as_object().cloned().unwrap_or_default();
    ensure!(
        !process.contains_key("confirmed") && !process.contains_key("acceptance"),
        "{r}"
    );
    let t = read(&pool, &ws, "task:t").await?;
    ensure!(
        t["process"]["status"] == "in_progress" && t["process"]["assignee"] == "alex",
        "{t}"
    );
    Ok(())
}
