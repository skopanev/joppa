//! requirement:file-upload, the store of attachment bytes: Spaces keeps each file once in the
//! folder of its workspace, `workspaces/<workspace>/<sha256>`, at most 50 MiB, never empty.
//! PostgreSQL keeps no file bytes.
#[path = "common/fake_spaces.rs"]
mod fake_spaces;
use anyhow::{Result, ensure};
use joppa::{
    spaces::{self, Spaces},
    store,
};

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
    let ws = format!("files-{}", uuid::Uuid::new_v4().simple());
    let other = format!("other-{}", uuid::Uuid::new_v4().simple());
    let bytes = format!("attachment {ws}").into_bytes();
    let (first, size) = storage.put(&ws, &bytes).await?;
    let (second, _) = storage.put(&ws, &bytes).await?;
    ensure!(first == second, "{first} / {second}");
    ensure!(
        first == joppa::digest_hex::sha256_hex(&bytes),
        "the key is not the SHA-256 hash"
    );
    ensure!(size == bytes.len(), "size {size}");
    let key = Spaces::key(&ws, &first)?;
    ensure!(key == format!("workspaces/{ws}/{first}"), "{key}");
    ensure!(
        fake.bytes(&ws, &first) == Some(bytes.clone()),
        "Spaces has other bytes"
    );
    ensure!(
        storage.size(&key).await? == Some(bytes.len() as u64),
        "HEAD size"
    );
    // Presigned URLs: AWS4-HMAC-SHA256 in the query, 900 s for GET and PUT, 60 s for HEAD.
    let get = query(&storage.download_url(&key, "a b.txt", "text/plain"));
    ensure!(get["X-Amz-Algorithm"] == "AWS4-HMAC-SHA256" && get["X-Amz-Expires"] == "900");
    ensure!(get["response-content-disposition"] == "inline; filename*=UTF-8''a%20b.txt");
    ensure!(query(&storage.url("PUT", &key, spaces::TRANSFER_TTL, &[]))["X-Amz-Expires"] == "900");
    ensure!(query(&storage.url("HEAD", &key, spaces::CHECK_TTL, &[]))["X-Amz-Expires"] == "60");
    ensure!(
        spaces::ATTACHMENT_MAX == 50 * 1024 * 1024,
        "the limit changed"
    );
    let over = vec![7u8; spaces::ATTACHMENT_MAX + 1];
    ensure!(
        storage.put(&ws, &over).await.is_err(),
        "50 MiB and 1 byte passed"
    );
    ensure!(
        !fake.any(&joppa::digest_hex::sha256_hex(&over)),
        "the larger file is in Spaces"
    );
    ensure!(storage.put(&ws, &[]).await.is_err(), "an empty file passed");
    ensure!(
        Spaces::folder("a/b").is_err(),
        "a workspace ID with a slash named a folder"
    );
    storage.delete(&key).await?;
    ensure!(storage.size(&key).await?.is_none(), "DELETE kept the file");
    // The folder of a workspace: three files over two list pages leave, another folder stays.
    for text in ["one", "two", "three"] {
        storage.put(&ws, text.as_bytes()).await?;
    }
    let (kept, _) = storage.put(&other, b"one").await?;
    ensure!(
        storage.delete_folder(&ws).await? == 3,
        "the folder had other files"
    );
    ensure!(
        storage.list(&Spaces::folder(&ws)?).await?.is_empty(),
        "the folder stays"
    );
    ensure!(fake.has(&other, &kept), "the other folder lost its file");
    let table: Option<String> = pool
        .get()
        .await?
        .query_one("SELECT to_regclass('joppa.blobs')::text", &[])
        .await?
        .get(0);
    ensure!(table.is_none(), "PostgreSQL still has joppa.blobs");
    Ok(())
}
