//! requirement:file-upload over a real server: an upload of exactly 10 MB is stored, and an
//! upload of 10 MB and 1 byte is refused and stores no blob.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::json;

// file-upload-limit
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_upload_over_10_mb_is_refused_and_stores_nothing() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("upload-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Uploads"}),
    )
    .await?;
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    // Unique bytes, so that an earlier test cannot have stored the same blob.
    let seed = uuid::Uuid::new_v4();
    let file =
        |len: usize| -> Vec<u8> { seed.as_bytes().iter().cycle().take(len).copied().collect() };
    let upload = |bytes: Vec<u8>| {
        let (root, ws) = (server.root.clone(), ws.clone());
        async move {
            reqwest::Client::new()
                .post(format!("{root}/api/workspaces/{ws}/blobs"))
                .bearer_auth(world::token("bot"))
                .body(bytes)
                .send()
                .await
                .map(|r| r.status().as_u16())
        }
    };
    let stored = |bytes: &[u8]| {
        let (pool, hash) = (pool.clone(), joppa::digest_hex::sha256_hex(bytes));
        async move {
            Ok::<i64, anyhow::Error>(
                pool.get()
                    .await?
                    .query_one("SELECT count(*) FROM joppa.blobs WHERE hash=$1", &[&hash])
                    .await?
                    .get(0),
            )
        }
    };
    let limit = store::ATTACHMENT_MAX;
    ensure!(limit == 10 * 1024 * 1024, "the limit is {limit} bytes");
    let exact = file(limit);
    let status = upload(exact.clone()).await?;
    ensure!(
        status == 200 && stored(&exact).await? == 1,
        "10 MB upload: HTTP {status}"
    );
    let over = file(limit + 1);
    let status = upload(over.clone()).await.unwrap_or(413);
    ensure!(status == 413, "the larger upload got HTTP {status}");
    ensure!(stored(&over).await? == 0, "the larger upload stored a blob");
    Ok(())
}
