//! The indexer gives each search row a vector. One drainer runs. A write commit, the server
//! start, a retry pass every 5 minutes and a reconcile pass every hour wake it. A wake that
//! comes during a drain is kept by the Notify permit, so no wake is lost.
//!
//! The indexer holds no database connection during a provider request, saves a vector only
//! when the row still has the content hash of the text that it sent, and copies a vector
//! that another row with the same hash already has. HTTP 400, 413 or 422 adds one attempt to
//! the text; after 5 attempts the indexer skips it. Other errors add no attempt.
use super::*;
use crate::embeddings::{self, Embedder, PROFILE};
use crate::embeddings_limit::{InputFault, ProviderStatus};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Notify;

pub const MAX_ATTEMPTS: i32 = 5;
pub const RETRY_EVERY: Duration = Duration::from_secs(300);
pub const RECONCILE_EVERY: Duration = Duration::from_secs(3600);
const BATCH: usize = 16;

static WAKE: OnceLock<Arc<Notify>> = OnceLock::new();

/// Wake the indexer after a write commit. Before the indexer starts, this does nothing.
pub fn wake() {
    if let Some(notify) = WAKE.get() {
        notify.notify_one();
    }
}

pub async fn run(pool: Pool, embedder: Embedder) {
    let notify = WAKE.get_or_init(|| Arc::new(Notify::new())).clone();
    let mut retry =
        tokio::time::interval_at(tokio::time::Instant::now() + RETRY_EVERY, RETRY_EVERY);
    let mut reconcile_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + RECONCILE_EVERY,
        RECONCILE_EVERY,
    );
    let mut full = true;
    loop {
        if full {
            log(reconcile(&pool).await);
        }
        log(drain(&pool, &embedder).await.map(|_| ()));
        tokio::select! {
            _ = notify.notified() => full = false,
            _ = retry.tick() => full = false,
            _ = reconcile_tick.tick() => full = true,
        }
    }
}

fn log(result: Result<()>) {
    if let Err(error) = result {
        eprintln!("joppa: search indexing waits for the next pass: {error:#}");
    }
}

/// Bring the search rows of each workspace up to the journal, with every object revision.
pub async fn reconcile(pool: &Pool) -> Result<()> {
    let ids: Vec<String> = pool
        .get()
        .await?
        .query("SELECT id FROM joppa.workspaces ORDER BY id", &[])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    for id in ids {
        let workspace = super::search::current(pool, &id, None).await?;
        super::search_revisions::project(&**pool.get().await?, &workspace, None).await?;
    }
    Ok(())
}

/// Embed rows until no pending row is left. Returns the number of rows that got a vector or
/// an attempt. A provider error that is not an input fault stops the drain.
pub async fn drain(pool: &Pool, embedder: &Embedder) -> Result<usize> {
    let mut total = 0;
    loop {
        let done = pass(pool, embedder).await?;
        if done == 0 {
            return Ok(total);
        }
        total += done;
    }
}

#[derive(Clone)]
struct Row {
    table: &'static str,
    workspace: String,
    key: String,
    revision: i32,
    input: String,
    hash: String,
}

const REUSE: [&str; 2] = [
    "UPDATE joppa.search_documents t SET embedding=s.embedding,embedding_profile=s.embedding_profile FROM (SELECT DISTINCT ON (content_hash) content_hash,embedding,embedding_profile FROM (SELECT content_hash,embedding,embedding_profile FROM joppa.search_documents WHERE embedding IS NOT NULL AND embedding_profile=$1 UNION ALL SELECT content_hash,embedding,embedding_profile FROM joppa.search_revisions WHERE embedding IS NOT NULL AND embedding_profile=$1) v ORDER BY content_hash) s WHERE t.content_hash=s.content_hash AND (t.embedding IS NULL OR t.embedding_profile IS DISTINCT FROM $1)",
    "UPDATE joppa.search_revisions t SET embedding=s.embedding,embedding_profile=s.embedding_profile FROM (SELECT DISTINCT ON (content_hash) content_hash,embedding,embedding_profile FROM (SELECT content_hash,embedding,embedding_profile FROM joppa.search_documents WHERE embedding IS NOT NULL AND embedding_profile=$1 UNION ALL SELECT content_hash,embedding,embedding_profile FROM joppa.search_revisions WHERE embedding IS NOT NULL AND embedding_profile=$1) v ORDER BY content_hash) s WHERE t.content_hash=s.content_hash AND (t.embedding IS NULL OR t.embedding_profile IS DISTINCT FROM $1)",
];

async fn pending(pool: &Pool) -> Result<Vec<Row>> {
    let conn = pool.get().await?;
    for sql in REUSE {
        conn.execute(sql, &[&PROFILE]).await?;
    }
    let rows = conn.query("SELECT 'search_documents',workspace,kind||':'||local_id,0,title,content,content_hash FROM joppa.search_documents WHERE (embedding IS NULL OR embedding_profile IS DISTINCT FROM $1) AND attempts<$2
        UNION ALL SELECT 'search_revisions',workspace,address,revision,title,content,content_hash FROM joppa.search_revisions WHERE (embedding IS NULL OR embedding_profile IS DISTINCT FROM $1) AND attempts<$2
        ORDER BY 1,2,3,4 LIMIT $3", &[&PROFILE, &MAX_ATTEMPTS, &((BATCH * crate::embeddings_limit::CONCURRENCY) as i64)]).await?;
    // The connection goes back to the pool before any provider request.
    drop(conn);
    Ok(rows
        .iter()
        .map(|r| Row {
            table: if r.get::<_, String>(0) == "search_documents" {
                "search_documents"
            } else {
                "search_revisions"
            },
            workspace: r.get(1),
            key: r.get(2),
            revision: r.get(3),
            input: super::search::embedding_input(r.get(4), r.get(5)),
            hash: r.get(6),
        })
        .collect())
}

/// One pass: up to 4 batches of 16 pending rows. Returns the rows that it settled.
pub async fn pass(pool: &Pool, embedder: &Embedder) -> Result<usize> {
    let rows = pending(pool).await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let mut set = tokio::task::JoinSet::new();
    for chunk in rows.chunks(BATCH) {
        let (pool, embedder, chunk) = (pool.clone(), embedder.clone(), chunk.to_vec());
        set.spawn(async move { batch(&pool, &embedder, &chunk).await });
    }
    let mut done = 0;
    let mut failure = None;
    while let Some(result) = set.join_next().await {
        match result? {
            Ok(n) => done += n,
            Err(error) => failure = Some(error),
        }
    }
    match failure {
        Some(error) if done == 0 => Err(error),
        _ => Ok(done),
    }
}

/// Embed one batch. When the provider answers the batch with an error status, embed each
/// text alone, so that only the refused text gets an attempt and the other texts get their
/// vectors. A transport error stops the pass: the provider is not reachable.
async fn batch(pool: &Pool, embedder: &Embedder, rows: &[Row]) -> Result<usize> {
    let inputs = rows.iter().map(|r| r.input.clone()).collect::<Vec<_>>();
    match embedder.embed(&inputs).await {
        Ok(vectors) => {
            for (row, vector) in rows.iter().zip(vectors) {
                save(pool, row, &vector).await?;
            }
            Ok(rows.len())
        }
        Err(error) if answered(&error) && rows.len() > 1 => {
            let mut done = 0;
            for row in rows {
                done += single(pool, embedder, row).await?;
            }
            Ok(done)
        }
        Err(error) => settle(pool, &rows[0], error).await,
    }
}

async fn single(pool: &Pool, embedder: &Embedder, row: &Row) -> Result<usize> {
    match embedder.embed(std::slice::from_ref(&row.input)).await {
        Ok(vectors) => {
            save(pool, row, &vectors[0]).await?;
            Ok(1)
        }
        Err(error) => settle(pool, row, error).await,
    }
}

/// An input fault adds one attempt. Another error status leaves the text for a later pass.
async fn settle(pool: &Pool, row: &Row, error: anyhow::Error) -> Result<usize> {
    if let Some(fault) = error.downcast_ref::<InputFault>() {
        attempt(pool, row, &fault.to_string()).await?;
        return Ok(1);
    }
    if error.downcast_ref::<ProviderStatus>().is_some() {
        return Ok(0);
    }
    Err(error)
}

fn answered(error: &anyhow::Error) -> bool {
    error.downcast_ref::<InputFault>().is_some() || error.downcast_ref::<ProviderStatus>().is_some()
}

fn target(row: &Row) -> (String, &'static str) {
    if row.table == "search_documents" {
        (
            "workspace=$1 AND kind||':'||local_id=$2 AND content_hash=$3 AND $4::int=0".into(),
            "search_documents",
        )
    } else {
        (
            "workspace=$1 AND address=$2 AND content_hash=$3 AND revision=$4".into(),
            "search_revisions",
        )
    }
}

async fn save(pool: &Pool, row: &Row, vector: &[f32]) -> Result<()> {
    let (filter, table) = target(row);
    pool.get().await?.execute(&format!("UPDATE joppa.{table} SET embedding=$5::text::vector,embedding_profile=$6,last_error=NULL WHERE {filter}"),
        &[&row.workspace, &row.key, &row.hash, &row.revision, &embeddings::literal(vector)?, &PROFILE]).await?;
    Ok(())
}

async fn attempt(pool: &Pool, row: &Row, error: &str) -> Result<()> {
    let (filter, table) = target(row);
    pool.get()
        .await?
        .execute(
            &format!("UPDATE joppa.{table} SET attempts=attempts+1,last_error=$5 WHERE {filter}"),
            &[&row.workspace, &row.key, &row.hash, &row.revision, &error],
        )
        .await?;
    Ok(())
}
