//! requirement:hybrid-search, the indexer in this process with a test provider: start and
//! reconcile, the input fault rule, the provider limits, and the release of the connection
//! with the content hash compare-and-set. The tests run one at a time, because a drain
//! takes every pending row of the shared database.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::{embeddings::Embedder, store, store::search_indexer};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

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

async fn workspace(pool: &deadpool_postgres::Pool, titles: &[&str]) -> Result<String> {
    let ws = format!("indexer-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Indexer"}),
    )
    .await?;
    for (i, title) in titles.iter().enumerate() {
        save(pool, &ws, &format!("d{i}"), title, 0).await?;
    }
    Ok(ws)
}

async fn save(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    id: &str,
    title: &str,
    revision: u32,
) -> Result<()> {
    store::execute_input(pool, ws, "owner", &uuid::Uuid::new_v4().to_string(), &json!({"op":"save_object","kind":"domain","id":id,"revision":revision,"parent":null,"data":{"title":title,"owner":"owner"},"reason":"Indexer test text"})).await?;
    Ok(())
}

/// Each search row of the workspace: (title, attempts, has a vector, current profile).
async fn rows(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Vec<(String, i32, bool, bool)>> {
    Ok(pool.get().await?.query("SELECT title,attempts,embedding IS NOT NULL,embedding_profile IS NOT DISTINCT FROM $2 FROM joppa.search_documents WHERE workspace=$1
        UNION ALL SELECT title,attempts,embedding IS NOT NULL,embedding_profile IS NOT DISTINCT FROM $2 FROM joppa.search_revisions WHERE workspace=$1", &[&ws, &joppa::embeddings::PROFILE]).await?
        .iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect())
}

// hybrid-search-reconcile
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn start_and_reconcile_embed_every_object_without_a_vector() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let ws = workspace(&pool, &["Alpha plan", "Beta plan", "Invoice plan"]).await?;
    let (_fake, endpoint) = world::provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    // The start: the indexer reconciles and drains with no write.
    let run = tokio::spawn(search_indexer::run(pool.clone(), embedder.clone()));
    let start = Instant::now();
    while rows(&pool, &ws).await?.iter().any(|r| !r.2) {
        ensure!(
            start.elapsed() < Duration::from_secs(20),
            "the start left rows without a vector"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // A reconcile pass renews an old profile and a missing revision row.
    let conn = pool.get().await?;
    conn.execute(
        "UPDATE joppa.search_documents SET embedding_profile='old-profile' WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    conn.execute(
        "DELETE FROM joppa.search_revisions WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    drop(conn);
    search_indexer::reconcile(&pool).await?;
    search_indexer::drain(&pool, &embedder).await?;
    let after = rows(&pool, &ws).await?;
    run.abort();
    ensure!(
        after.len() == 6 && after.iter().all(|r| r.2 && r.3),
        "{after:?}"
    );
    Ok(())
}

// hybrid-search-input-fault
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_rejected_text_stops_after_five_attempts_and_blocks_no_other() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let ws = workspace(
        &pool,
        &["rejectme plan", "flakyword plan", "Alpha plan", "Beta plan"],
    )
    .await?;
    let (_fake, endpoint) = world::provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    for _ in 0..8 {
        search_indexer::drain(&pool, &embedder).await?;
    }
    for (title, attempts, vector, _) in rows(&pool, &ws).await? {
        let ok = match title.as_str() {
            "rejectme plan" => attempts == 5 && !vector,
            "flakyword plan" => attempts == 0 && !vector,
            _ => attempts == 0 && vector,
        };
        ensure!(ok, "{title}: attempts {attempts}, vector {vector}");
    }
    // The texts that keep failing leave the shared queue, so a later test drains only its own.
    let conn = pool.get().await?;
    conn.execute(
        "DELETE FROM joppa.search_documents WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    conn.execute(
        "DELETE FROM joppa.search_revisions WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    Ok(())
}

// hybrid-search-rate
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_indexer_sends_at_most_4_requests_at_once_and_200_a_minute() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let titles: Vec<String> = (0..300).map(|i| format!("Plan number {i}")).collect();
    let ws = workspace(
        &pool,
        &titles.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .await?;
    let (fake, endpoint) = world::provider().await?;
    fake.delay_ms.store(200, Ordering::SeqCst);
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    search_indexer::drain(&pool, &embedder).await?;
    ensure!(
        rows(&pool, &ws).await?.iter().all(|r| r.2),
        "rows without a vector"
    );
    let max_open = fake.max_open.load(Ordering::SeqCst);
    let starts = fake.starts.lock().unwrap().clone();
    let widest = starts
        .iter()
        .map(|t| {
            starts
                .iter()
                .filter(|u| **u >= *t && **u < *t + Duration::from_secs(60))
                .count()
        })
        .max()
        .unwrap_or(0);
    ensure!(max_open == 4, "open requests at one time: {max_open}");
    ensure!(widest <= 200, "requests in one 60-second window: {widest}");
    println!(
        "hybrid-search-rate: {} requests, at most {max_open} open, at most {widest} in 60 s",
        starts.len()
    );
    Ok(())
}

// hybrid-search-stale-vector
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_slow_provider_holds_no_connection_and_saves_no_stale_vector() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = db().await?;
    let (fake, endpoint) = world::provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    // Only the rows of this workspace wait for a vector.
    search_indexer::drain(&pool, &embedder).await?;
    let ws = workspace(&pool, &["Alpha plan"]).await?;
    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(std::env::var("JOPPA_TEST_DATABASE_URL")?);
    cfg.pool = Some(deadpool_postgres::PoolConfig::new(1));
    let one = cfg.create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?;
    fake.hold_first.store(true, Ordering::SeqCst);
    let (p, e) = (one.clone(), embedder.clone());
    let pass = tokio::spawn(async move { search_indexer::pass(&p, &e).await });
    fake.held.notified().await;
    let start = Instant::now();
    save(&one, &ws, "d0", "Beta plan", 1).await?;
    let change = start.elapsed();
    tokio::time::sleep(Duration::from_secs(2)).await;
    fake.release.notify_one();
    pass.await??;
    let stale: bool = pool.get().await?.query_one("SELECT embedding IS NULL FROM joppa.search_documents WHERE workspace=$1 AND local_id='d0'", &[&ws]).await?.get(0);
    search_indexer::drain(&pool, &embedder).await?;
    let text: String = pool.get().await?.query_one("SELECT embedding::text FROM joppa.search_documents WHERE workspace=$1 AND local_id='d0'", &[&ws]).await?.get(0);
    let vector: Vec<f32> = serde_json::from_str(&text)?;
    ensure!(
        change < Duration::from_secs(1),
        "the change waited {change:?} for the provider"
    );
    ensure!(
        stale,
        "the vector of the old text was saved for the new text"
    );
    ensure!(
        vector[world::dimension("beta")] > 0.99,
        "the new text has another vector"
    );
    Ok(())
}
