//! requirement:file-upload, the store of attachment bytes: Spaces keeps each file once under
//! `files/<sha256>`, at most 50 MiB, never empty. PostgreSQL keeps no file bytes.
#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Result, ensure};
use joppa::{spaces, store};

fn query(url: &str) -> std::collections::BTreeMap<String, String> {
    let url = reqwest::Url::parse(url).unwrap();
    url.query_pairs().into_owned().collect()
}

// attachment-bytes-in-spaces
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_upload_stores_its_bytes_once_in_spaces_under_their_sha_256() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let fake = fake_spaces::start();
    fake.configure();
    let storage = spaces::storage()?;
    // Unique bytes, so that an earlier run cannot have stored the same file.
    let bytes = format!("attachment {}", uuid::Uuid::new_v4()).into_bytes();
    let (first, size) = storage.put(&bytes).await?;
    let (second, _) = storage.put(&bytes).await?;
    ensure!(first == second, "{first} / {second}");
    ensure!(
        first == joppa::digest_hex::sha256_hex(&bytes),
        "the key is not the SHA-256 hash"
    );
    ensure!(size == bytes.len(), "size {size}");
    ensure!(
        fake.bytes(&first) == Some(bytes.clone()),
        "Spaces has other bytes"
    );
    ensure!(
        storage.size(&first).await? == Some(bytes.len() as u64),
        "HEAD size"
    );
    // Presigned URLs: AWS4-HMAC-SHA256 in the query, 900 s for GET and PUT, 60 s for HEAD.
    let get = query(&storage.download_url(&first, "a b.txt", "text/plain"));
    ensure!(get["X-Amz-Algorithm"] == "AWS4-HMAC-SHA256" && get["X-Amz-Expires"] == "900");
    ensure!(get["response-content-disposition"] == "inline; filename*=UTF-8''a%20b.txt");
    ensure!(
        query(&storage.url("PUT", &first, spaces::TRANSFER_TTL, &[]))["X-Amz-Expires"] == "900"
    );
    ensure!(query(&storage.url("HEAD", &first, spaces::CHECK_TTL, &[]))["X-Amz-Expires"] == "60");
    ensure!(
        spaces::ATTACHMENT_MAX == 50 * 1024 * 1024,
        "the limit changed"
    );
    let over = vec![7u8; spaces::ATTACHMENT_MAX + 1];
    ensure!(
        storage.put(&over).await.is_err(),
        "50 MiB and 1 byte passed"
    );
    let over_hash = joppa::digest_hex::sha256_hex(&over);
    ensure!(!fake.has(&over_hash), "the larger file is in Spaces");
    ensure!(storage.put(&[]).await.is_err(), "an empty file passed");
    storage.delete(&first).await?;
    ensure!(
        storage.size(&first).await?.is_none(),
        "DELETE kept the file"
    );
    let table: Option<String> = pool
        .get()
        .await?
        .query_one("SELECT to_regclass('joppa.blobs')::text", &[])
        .await?
        .get(0);
    ensure!(table.is_none(), "PostgreSQL still has joppa.blobs");
    Ok(())
}
