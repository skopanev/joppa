//! requirement:file-upload, the store of attachment bytes: PostgreSQL table joppa.blobs keeps
//! each file once under the SHA-256 hash of its bytes, at most 10 MB, never empty.
use anyhow::{Result, ensure};
use joppa::store;

async fn db() -> Result<deadpool_postgres::Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
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
    Ok(pool)
}

// file-upload-store
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_upload_stores_its_bytes_once_under_their_sha_256() -> Result<()> {
    let pool = db().await?;
    // Unique bytes, so that an earlier run cannot have stored the same file.
    let bytes = format!("attachment {}", uuid::Uuid::new_v4()).into_bytes();
    let (first, size) = store::put_blob(&pool, &bytes).await?;
    let (second, _) = store::put_blob(&pool, &bytes).await?;
    ensure!(first == second, "{first} / {second}");
    ensure!(
        first == joppa::digest_hex::sha256_hex(&bytes),
        "the key is not the SHA-256 hash"
    );
    ensure!(size == bytes.len(), "size {size}");
    let rows = pool
        .get()
        .await?
        .query(
            "SELECT bytes,size FROM joppa.blobs WHERE hash=$1",
            &[&first],
        )
        .await?;
    ensure!(rows.len() == 1, "{} rows", rows.len());
    let stored: Vec<u8> = rows[0].get(0);
    let stored_size: i64 = rows[0].get(1);
    ensure!(stored == bytes, "the stored bytes differ");
    ensure!(
        stored_size == bytes.len() as i64,
        "stored size {stored_size}"
    );
    ensure!(
        store::ATTACHMENT_MAX == 10 * 1024 * 1024,
        "the limit changed"
    );
    let over = vec![7u8; store::ATTACHMENT_MAX + 1];
    ensure!(
        store::put_blob(&pool, &over).await.is_err(),
        "10 MB and 1 byte passed"
    );
    ensure!(
        store::put_blob(&pool, &[]).await.is_err(),
        "an empty file passed"
    );
    Ok(())
}
