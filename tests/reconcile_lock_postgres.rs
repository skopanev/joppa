//! The hourly reconcile pass of a workspace waits for the write lock of that workspace.
#[path = "common/dims_tree.rs"]
mod dims_tree;
use anyhow::{Result, ensure};
use dims_tree::{Pool, tree};
use joppa::store::{self, search_indexer};
use std::time::Duration;

/// The revisions of `ws` and the search rows of them.
async fn counts(pool: &Pool, ws: &str) -> Result<(i64, i64)> {
    let sql = "SELECT (SELECT count(*) FROM joppa.object_revisions WHERE workspace=$1),(SELECT count(*) FROM joppa.search_revisions WHERE workspace=$1)";
    let row = pool.get().await?.query_one(sql, &[&ws]).await?;
    Ok((row.get(0), row.get(1)))
}

// ac:lxh0a149
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_reconcile_pass_waits_for_the_write_lock_of_its_workspace() -> Result<()> {
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
    let (ws, _) = tree(&pool, 1, 1).await?;
    let mut conn = pool.get().await?;
    conn.execute(
        "DELETE FROM joppa.search_revisions WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    let (revisions, rows) = counts(&pool, &ws).await?;
    ensure!(revisions > 0 && rows == 0, "{revisions} {rows}");
    // A transaction holds the write lock of the workspace, as a write does.
    let tx = conn.transaction().await?;
    let lock = "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))";
    tx.execute(lock, &[&ws]).await?;
    let pass = tokio::spawn({
        let pool = pool.clone();
        async move { search_indexer::reconcile(&pool).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    ensure!(
        !pass.is_finished(),
        "the pass ended while the lock was held"
    );
    tx.commit().await?;
    pass.await??;
    let (revisions, rows) = counts(&pool, &ws).await?;
    ensure!(
        rows == revisions,
        "{revisions} revisions, {rows} search rows"
    );
    Ok(())
}
