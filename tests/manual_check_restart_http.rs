//! requirement:jjhid61n over a real server: a pass with valid_until today (UTC) is grey at
//! once, and stays grey when the server restarts and rebuilds the state from the journal.
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use manual_check::{check, day, expect};
use serde_json::json;

/// The projections that a server start rebuilds from the journal.
const TABLES: [&str; 7] = [
    "objects",
    "object_revisions",
    "object_links",
    "state_entries",
    "state_revisions",
    "search_documents",
    "board_cache",
];

// ac:65ycqdmb
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_pass_with_valid_until_today_is_grey_at_once_and_after_a_restart() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    manual_check::criterion(&w, "g", &[]).await?;
    let fields = json!({"check_result":"pass","valid_until":day(0)});
    check(&w, "ac:g", fields).await?;
    let expired = Some("Manual check expired");
    expect(&w, "right after the attach", "ac:g", "grey", expired).await?;
    let before = manual_check::state(&w, "ac:g").await?;
    // Stop the server, remove the projections of the workspace and keep the journal.
    w.stop();
    {
        let conn = w.pool.get().await?;
        for table in TABLES {
            let sql = format!("DELETE FROM joppa.{table} WHERE workspace=$1");
            conn.execute(&sql, &[&w.ws]).await?;
        }
    }
    w.start().await?;
    let sql = "SELECT count(*) FROM joppa.board_cache WHERE workspace=$1";
    let rebuilt: i64 = w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0);
    ensure!(rebuilt == 1, "the start did not rebuild the workspace");
    expect(&w, "after the restart", "ac:g", "grey", expired).await?;
    let after = manual_check::state(&w, "ac:g").await?;
    ensure!(
        after == before,
        "before the restart {before:?}, after {after:?}"
    );
    Ok(())
}
