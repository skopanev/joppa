//! The hourly reconcile pass of a workspace waits only for the search lock of that workspace,
//! and a write never waits for the pass.
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
async fn the_reconcile_pass_waits_for_the_search_lock_and_no_write_waits_for_it() -> Result<()> {
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
    // A transaction holds the search lock of the workspace, as a search builder does.
    let tx = conn.transaction().await?;
    let lock = "SELECT pg_advisory_xact_lock(hashtextextended($1,91827103))";
    tx.execute(lock, &[&ws]).await?;
    let pass = tokio::spawn({
        let pool = pool.clone();
        async move { search_indexer::reconcile(&pool).await }
    });
    // A save of a new object ends while the search lock is held.
    let save = serde_json::json!({"op":"save_object","kind":"domain","id":"later","revision":0,
        "parent":null,"data":{"title":"Later","owner":"owner"},"reason":"A write during the pass"});
    let id = uuid::Uuid::new_v4().to_string();
    let write = store::execute_remote_input(&pool, &ws, "owner", &id, &save);
    tokio::time::timeout(Duration::from_secs(5), write).await??;
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
