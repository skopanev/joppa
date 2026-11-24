// Part 2 of tests/search_step_postgres.rs: boundary move-only marker and monotonic position check.

// ac:6zvnk4k2
// test:5oc8z85h
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn move_marker_on_cold_cache_advances_search_state_without_loading_rows_or_regressing_position()
-> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    setup_domain_schema(&w).await?;
    w.mcp(&w.bot, "joppa_search", json!({"query":"R"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let conn = w.pool.get().await?;
    conn.batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .await?;
    let p: i64 = conn
        .query_one("SELECT position FROM joppa.search_state WHERE workspace=$1", &[&w.ws])
        .await?
        .get(0);

    // Put search_state ahead at P + 10 (as after a concurrent record_report) and queue "#" at P + 1.
    conn.execute(
        "UPDATE joppa.search_state SET position=$2 WHERE workspace=$1",
        &[&w.ws, &(p + 10)],
    )
    .await?;
    conn.execute(
        "INSERT INTO joppa.search_pending(workspace,address,position) VALUES($1,'#',$2)",
        &[&w.ws, &p],
    )
    .await?;
    conn.execute("UPDATE joppa.board_cache SET position=position WHERE workspace=$1", &[&w.ws])
        .await?;
    conn.query_one(
        "SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)",
        &[],
    )
    .await?;

    let hit = w
        .mcp(&w.bot, "joppa_search", json!({"query":"R"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(hit["items"].as_array().is_some_and(|a| !a.is_empty()), "{hit}");

    let row = conn
        .query_one(
            "SELECT (SELECT position FROM joppa.search_state WHERE workspace=$1), (SELECT count(*) FROM joppa.search_pending WHERE workspace=$1)",
            &[&w.ws],
        )
        .await?;
    let (after_pos, pending_left): (i64, i64) = (row.get(0), row.get(1));
    ensure!(after_pos == p + 10 && pending_left == 0, "after_pos={after_pos}, pending={pending_left}");

    let stmts = conn
        .query(
            "SELECT lower(query), calls FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database())",
            &[],
        )
        .await?;
    let scans: i64 = stmts
        .iter()
        .filter(|r| r.get::<_, String>(0).contains("from joppa.state_entries s left join compact"))
        .map(|r| r.get::<_, i64>(1))
        .sum();
    ensure!(scans == 0, "move-only marker loaded state_entries {scans} times");
    Ok(())
}
