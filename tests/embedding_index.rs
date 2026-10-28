//! requirement:embedding-input and requirement:indexer-schedule, the indexer in this process
//! with a test provider: the input of 8000 characters and its SHA-256 hash, the reuse of a
//! known vector, the batch of 16 texts, the fallback to one text for each request, and the
//! fixed intervals and limits. The tests run one at a time, because a drain takes every
//! pending row of the shared database.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::{embeddings::Embedder, embeddings_limit, store, store::search_indexer};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

async fn workspace(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("input-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Embedding input"}),
    )
    .await?;
    Ok(ws)
}

/// Save a domain with this title and, when given, this description.
async fn save(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    id: &str,
    title: &str,
    description: Option<&str>,
) -> Result<()> {
    let mut data = json!({"title":title,"owner":"owner"});
    // Each field holds at most 1024 characters, so a long text goes into several fields.
    if let Some(description) = description {
        let chars: Vec<char> = description.chars().collect();
        for (i, part) in chars.chunks(1000).enumerate() {
            let key = if i == 0 {
                "description".to_owned()
            } else {
                format!("part{i}")
            };
            data[key] = json!(part.iter().collect::<String>());
        }
    }
    store::execute_input(pool, ws, "owner", &uuid::Uuid::new_v4().to_string(), &json!({"op":"save_object","kind":"domain","id":id,"revision":0,"parent":null,"data":data,"reason":"Embedding input test text"})).await?;
    Ok(())
}

/// The search rows of the workspace that have no vector of the current profile.
async fn missing(pool: &deadpool_postgres::Pool, ws: &str) -> Result<i64> {
    Ok(pool.get().await?.query_one("SELECT (SELECT count(*) FROM joppa.search_documents WHERE workspace=$1 AND (embedding IS NULL OR embedding_profile IS DISTINCT FROM $2))
        + (SELECT count(*) FROM joppa.search_revisions WHERE workspace=$1 AND (embedding IS NULL OR embedding_profile IS DISTINCT FROM $2))", &[&ws, &joppa::embeddings::PROFILE]).await?.get(0))
}

async fn drain(endpoint: &str, pool: &deadpool_postgres::Pool) -> Result<()> {
    let embedder = Embedder::new("test-only-key".into(), endpoint.into())?;
    search_indexer::drain(pool, &embedder).await?;
    Ok(())
}

// embedding-input-limit
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_provider_gets_the_first_8000_characters_and_the_hash_is_their_sha_256() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let ws = workspace(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    let title = format!("Long text {}", uuid::Uuid::new_v4().simple());
    save(&pool, &ws, "long", &title, Some(&"я".repeat(9000))).await?;
    drain(&endpoint, &pool).await?;
    let prefix = format!("{title}\n");
    let sent = fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .find(|text| text.starts_with(&prefix))
        .cloned();
    let Some(sent) = sent else {
        anyhow::bail!("the provider did not get the text")
    };
    ensure!(
        sent.chars().count() == 8000,
        "{} characters",
        sent.chars().count()
    );
    let hash: String = pool
        .get()
        .await?
        .query_one(
            "SELECT content_hash FROM joppa.search_documents WHERE workspace=$1 AND kind='domain' AND local_id='long'",
            &[&ws],
        )
        .await?
        .get(0);
    ensure!(hash == joppa::digest_hex::sha256_hex(&sent), "{hash}");
    Ok(())
}

// embedding-hash-reuse
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_text_with_a_known_hash_reuses_its_vector_without_a_request() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let ws = workspace(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    let title = format!("Twin text {}", uuid::Uuid::new_v4().simple());
    save(&pool, &ws, "first", &title, None).await?;
    drain(&endpoint, &pool).await?;
    ensure!(
        missing(&pool, &ws).await? == 0,
        "the first object has no vector"
    );
    fake.requests.lock().unwrap().clear();
    save(&pool, &ws, "second", &title, None).await?;
    drain(&endpoint, &pool).await?;
    let requests = fake.requests.lock().unwrap().clone();
    ensure!(
        !requests.iter().flatten().any(|text| text.contains(&title)),
        "the same text went to the provider again: {requests:?}"
    );
    ensure!(
        missing(&pool, &ws).await? == 0,
        "the second object has no vector"
    );
    Ok(())
}

// indexer-batch-16
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_pass_sends_batches_of_at_most_16_texts() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let ws = workspace(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    let run = uuid::Uuid::new_v4().simple().to_string();
    for i in 0..40 {
        save(
            &pool,
            &ws,
            &format!("d{i}"),
            &format!("Batch {run} {i}"),
            None,
        )
        .await?;
    }
    drain(&endpoint, &pool).await?;
    let sizes: Vec<usize> = fake.requests.lock().unwrap().iter().map(Vec::len).collect();
    ensure!(sizes.iter().all(|&n| (1..=16).contains(&n)), "{sizes:?}");
    ensure!(sizes.contains(&16), "no request held 16 texts: {sizes:?}");
    ensure!(missing(&pool, &ws).await? == 0, "rows without a vector");
    Ok(())
}

// indexer-batch-fallback
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_refused_batch_falls_back_to_one_request_for_each_text() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let ws = workspace(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    fake.refuse_batches.store(true, Ordering::SeqCst);
    let run = uuid::Uuid::new_v4().simple().to_string();
    let titles: Vec<String> = (0..5).map(|i| format!("Fallback {run} {i}")).collect();
    for (i, title) in titles.iter().enumerate() {
        save(&pool, &ws, &format!("f{i}"), title, None).await?;
    }
    drain(&endpoint, &pool).await?;
    let requests = fake.requests.lock().unwrap().clone();
    ensure!(
        requests.iter().any(|r| r.len() > 1),
        "no batch request: {requests:?}"
    );
    for title in &titles {
        ensure!(
            requests
                .iter()
                .any(|r| r.len() == 1 && r[0].starts_with(&format!("{title}\n"))),
            "{title} had no request of its own"
        );
    }
    ensure!(missing(&pool, &ws).await? == 0, "rows without a vector");
    Ok(())
}

// indexer-intervals
#[test]
fn the_retry_pass_runs_every_300_seconds_and_reconcile_every_3600() {
    assert_eq!(search_indexer::RETRY_EVERY, Duration::from_secs(300));
    assert_eq!(search_indexer::RECONCILE_EVERY, Duration::from_secs(3600));
    assert_eq!(search_indexer::MAX_ATTEMPTS, 5);
    assert_eq!(embeddings_limit::CONCURRENCY, 4);
    assert_eq!(embeddings_limit::PER_WINDOW, 200);
    assert_eq!(embeddings_limit::WINDOW, Duration::from_secs(60));
}
