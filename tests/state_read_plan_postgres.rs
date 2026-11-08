//! The read of the current rows of a workspace: its plan on a workspace with 2,000 rows. The
//! test takes the statement from src/store_state_current.rs at build time, so a change of the
//! statement in the code changes the statement that the test plans, and a statement that the
//! test cannot find fails the test.
#[path = "common/pg_plan.rs"]
mod pg_plan;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const SOURCE: &str = include_str!("../src/store_state_current.rs");

// ac:two3y4jo
// test:zoqkg4cz
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_current_rows_read_sorts_nothing_and_reads_the_board_row_once() -> Result<()> {
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "state-plan").await?;
    // 1,900 domains and 100 inbox notes, so the compact branch of the read has rows too.
    pg_plan::many_nodes(&pool, &ws, 1900, 100).await?;
    pg_plan::analyze(&pool).await?;
    let conn = pool.get().await?;
    let rows: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.state_entries WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    ensure!(rows >= 2000, "the workspace has {rows} current rows");
    let sql = pg_plan::literal(SOURCE, "FROM joppa.state_entries s")?;
    ensure!(!sql.to_uppercase().contains("ORDER BY"), "{sql}");
    let plan = pg_plan::explain(&**conn, "ANALYZE, BUFFERS, FORMAT JSON", &sql, &[&ws]).await?;
    let nodes = pg_plan::nodes(&plan);
    let evidence = json!({"rows":rows,"statement":sql,"plan":plan});
    println!("{evidence}");
    ensure!(
        !nodes.iter().any(|n| n["Node Type"] == "Sort"),
        "a Sort node: {evidence}"
    );
    let temp = |n: &Value| {
        n["Temp Written Blocks"].as_u64().unwrap_or(0) + n["Temp Read Blocks"].as_u64().unwrap_or(0)
    };
    ensure!(
        nodes.iter().all(|n| temp(n) == 0),
        "temporary blocks: {evidence}"
    );
    // The board row is read once for the statement, not once for each current row.
    let board: Vec<&Value> = nodes
        .iter()
        .filter(|n| n["Relation Name"] == "board_cache")
        .collect();
    ensure!(board.len() == 1, "board_cache nodes: {evidence}");
    ensure!(
        board[0]["Actual Loops"] == 1,
        "board_cache loops: {evidence}"
    );
    let types: Vec<&Value> = nodes
        .iter()
        .filter(|n| n["Function Name"] == "jsonb_each")
        .collect();
    ensure!(
        types.len() == 1 && types[0]["Actual Loops"] == 1,
        "the type list is read more than once: {evidence}"
    );
    // The plan reads every current row of the workspace.
    let entries: Vec<&Value> = nodes
        .iter()
        .filter(|n| n["Relation Name"] == "state_entries")
        .collect();
    ensure!(entries.len() == 1, "state_entries nodes: {evidence}");
    let read = entries[0]["Actual Rows"].as_i64().unwrap_or(0)
        * entries[0]["Actual Loops"].as_i64().unwrap_or(0);
    ensure!(read == rows, "{read} of {rows} rows: {evidence}");
    Ok(())
}
