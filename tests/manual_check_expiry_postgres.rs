//! requirement:jjhid61n: store::expire_manual_checks turns an expired manual check grey
//! without a journal event, and a run that changes no status writes no row.
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use manual_check::{check, expect};
use serde_json::json;

const EXPIRED: Option<&str> = Some("Manual check expired");

/// The xmin of each joppa.objects row of the workspace and of its board_cache row.
async fn xmins(w: &record_world::World) -> Result<Vec<(String, String)>> {
    let conn = w.pool.get().await?;
    let objects =
        "SELECT address, xmin::text FROM joppa.objects WHERE workspace=$1 ORDER BY address";
    let mut rows: Vec<(String, String)> = conn
        .query(objects, &[&w.ws])
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let board = "SELECT xmin::text FROM joppa.board_cache WHERE workspace=$1";
    let board: String = conn.query_one(board, &[&w.ws]).await?.get(0);
    rows.push(("board_cache".into(), board));
    Ok(rows)
}

// ac:x3l2e9l5
// test:h72tfyyn
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_expired_manual_check_turns_grey_at_the_next_check_run_without_a_write() -> Result<()> {
    let w = record_world::world(&[]).await?;
    manual_check::criterion(&w, "f", &[]).await?;
    // D is a date after today (UTC).
    let d = chrono::Utc::now().date_naive() + chrono::TimeDelta::days(2);
    let until = d.format("%Y-%m-%d").to_string();
    check(
        &w,
        "ac:a",
        json!({"check_result":"pass","valid_until":until}),
    )
    .await?;
    check(&w, "ac:f", json!({"check_result":"pass"})).await?;
    expect(&w, "given", "ac:a", "green", None).await?;
    expect(&w, "given", "ac:f", "green", None).await?;
    let position = w.position().await?;
    let before = xmins(&w).await?;
    let day_before = d.pred_opt().context("the day before D")?;
    let changed = joppa::store::expire_manual_checks(&w.pool, &w.ws, day_before).await?;
    ensure!(
        xmins(&w).await? == before,
        "the run with D - 1 changed a row"
    );
    ensure!(changed == 0, "the run with D - 1 returns {changed}");
    expect(&w, "the run with D - 1", "ac:a", "green", None).await?;
    let changed = joppa::store::expire_manual_checks(&w.pool, &w.ws, d).await?;
    ensure!(changed == 1, "the first run with D returns {changed}");
    expect(&w, "the first run with D", "ac:a", "grey", EXPIRED).await?;
    let r = manual_check::tone(&w, "requirement:r").await?;
    ensure!(r == "grey", "the first run with D: R must be grey: {r}");
    let after = xmins(&w).await?;
    let changed = joppa::store::expire_manual_checks(&w.pool, &w.ws, d).await?;
    ensure!(
        xmins(&w).await? == after,
        "the second run with D changed a row"
    );
    ensure!(changed == 0, "the second run with D returns {changed}");
    expect(&w, "the second run with D", "ac:a", "grey", EXPIRED).await?;
    expect(&w, "after the runs", "ac:f", "green", None).await?;
    ensure!(
        w.position().await? == position,
        "the runs added a journal event"
    );
    Ok(())
}
