//! requirement:hybrid-search over a real server: the excerpt of a result is the own saved
//! data of the object, never the text of its parents.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<()> {
    store::execute_input(
        pool,
        ws,
        "owner",
        &uuid::Uuid::new_v4().to_string(),
        &command,
    )
    .await?;
    Ok(())
}

// hybrid-search-excerpt
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn an_excerpt_gives_the_own_data_of_the_object_without_its_parents() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    store::migrate(&pool).await?;
    let ws = format!("excerpt-{}", uuid::Uuid::new_v4().simple());
    write(&pool, &ws, json!({"op":"workspace","title":"Excerpt"})).await?;
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    write(&pool, &ws, json!({"op":"save_object","kind":"domain","id":"p","revision":0,"parent":null,"data":{"title":"Parent","owner":"owner","body":"Parent context alpha"},"reason":"Excerpt test"})).await?;
    write(&pool, &ws, json!({"op":"save_object","kind":"capability","id":"c","revision":0,"parent":"domain:p","data":{"title":"Child","owner":"owner","body":"Old child text"},"reason":"Excerpt test"})).await?;
    write(&pool, &ws, json!({"op":"save_object","kind":"capability","id":"c","revision":1,"parent":"domain:p","data":{"title":"Child","owner":"owner","body":"Own child text bravo"},"reason":"Excerpt test"})).await?;
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    let found = world::mcp(
        &server.root,
        "bot",
        "joppa_search",
        json!({"workspace":ws,"query":"bravo"}),
    )
    .await?;
    let child = found["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| i["address"] == "capability:c")
        .cloned()
        .unwrap_or_default();
    ensure!(child["excerpt"] == "Own child text bravo", "{found}");
    // A criterion hit gives its own revision, not the revision of its requirement.
    write(&pool, &ws, json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":"capability:c","data":{"title":"Rule","owner":"owner","body":"Rule text"},"reason":"Excerpt test"})).await?;
    write(&pool, &ws, json!({"op":"save_object","kind":"requirement","id":"r","revision":1,"parent":"capability:c","data":{"title":"Rule","owner":"owner","body":"Rule text again"},"reason":"Excerpt test"})).await?;
    write(&pool, &ws, json!({"op":"save_object","kind":"consumer","id":"buyer","revision":0,"parent":null,"data":{"title":"Buyer","owner":"owner"},"reason":"Excerpt test"})).await?;
    write(&pool, &ws, json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"Charlie check","owner":"owner","consumer":"buyer","before_launch":false,"decision_required":false,"depends_on":[]},"reason":"Excerpt test"})).await?;
    let hits = world::mcp(
        &server.root,
        "bot",
        "joppa_search",
        json!({"workspace":ws,"query":"charlie"}),
    )
    .await?;
    ensure!(
        hits["items"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|i| i["address"] == "ac:a" && i["revision"] == 1 && i["req"] == "requirement:r"),
        "{hits}"
    );
    let old = world::mcp(
        &server.root,
        "bot",
        "joppa_search",
        json!({"workspace":ws,"query":"old","all_versions":true}),
    )
    .await?;
    ensure!(
        old["items"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|i| i["address"] == "capability:c"
                && i["revision"] == 1
                && i["excerpt"] == "Old child text"),
        "{old}"
    );
    Ok(())
}
