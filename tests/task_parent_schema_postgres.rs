//! The tree schema sets where a Task attaches: with parents ["*"] a Task attaches to a
//! Domain, and with parents ["ac"] Joppa refuses a Task under a Domain.
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

/// A workspace with member bot, Domain D and a Task type with the given parents.
async fn workspace(pool: &deadpool_postgres::Pool, task_parents: Value) -> Result<String> {
    let ws = format!("task-parent-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Task parents"}),
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
    write(pool, &ws, "owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure Task parents","types":{
        "domain":{"title":"Domains","role":"group","root":true,"parents":[]},
        "requirement":{"title":"Requirements","role":"requirement","parents":["domain"]},
        "ac":{"title":"Criteria","role":"ac","parents":["requirement"]},
        "task":{"title":"Tasks","role":"task","parents":task_parents}
    }})).await?;
    write(pool, &ws, "bot", json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"Domain D","owner":"owner"},"reason":"Create the domain"})).await?;
    Ok(ws)
}

fn task_under_d() -> Value {
    json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"domain:d","data":{"title":"Task T","owner":"owner","contract":"Implement the domain change","purpose":"implementation","projects":[]},"reason":"Create work"})
}

async fn children_of_d(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Vec<String>> {
    let page = store::read_objects(
        pool,
        ws,
        "bot",
        &store::ObjectScope {
            parent: Some("domain:d"),
            limit: 100,
            ..Default::default()
        },
    )
    .await?;
    Ok(page["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect())
}

async fn head(pool: &deadpool_postgres::Pool, ws: &str) -> Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_attaches_to_a_domain_when_the_schema_allows_it() -> Result<()> {
    // ac:11547d51: the Task type has parents ["*"].
    let pool = pool().await?;
    let ws = workspace(&pool, json!(["*"])).await?;
    write(&pool, &ws, "bot", task_under_d()).await?;
    let task = store::read_objects(
        &pool,
        &ws,
        "bot",
        &store::ObjectScope {
            address: Some("task:t"),
            ..Default::default()
        },
    )
    .await?;
    ensure!(task["document"]["parent"] == "domain:d", "{task}");
    ensure!(
        children_of_d(&pool, &ws).await?.contains(&"task:t".into()),
        "the children of D include T"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_parent_outside_the_schema_is_refused() -> Result<()> {
    // ac:796b8b22: the Task type has parents ["ac"].
    let pool = pool().await?;
    let ws = workspace(&pool, json!(["ac"])).await?;
    let position = head(&pool, &ws).await?;
    let children = children_of_d(&pool, &ws).await?;
    write(&pool, &ws, "bot", task_under_d())
        .await
        .expect_err("a Task under a Domain must be refused");
    ensure!(
        head(&pool, &ws).await? == position,
        "the journal must not grow"
    );
    ensure!(
        children_of_d(&pool, &ws).await? == children,
        "D keeps its children"
    );
    Ok(())
}
