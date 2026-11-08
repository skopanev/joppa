//! The owner read of the workspaces that name a workspace: the owner check reads one row and
//! loads no current state, and the lookup of the naming workspaces reads indexes. The lookup
//! statement comes from src/store_references.rs at build time, so the test plans the statement
//! of the code. The tests read a process-wide probe counter: they run one at a time.
#[path = "common/pg_plan.rs"]
mod pg_plan;
use anyhow::{Result, ensure};
use joppa::{probe, store};
use serde_json::json;

const SOURCE: &str = include_str!("../src/store_references.rs");

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ac:hqp77qd2
// test:8uwjimbd
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_references_read_of_an_unnamed_workspace_loads_no_current_state() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "refs-alone").await?;
    pg_plan::many_nodes(&pool, &ws, 50, 5).await?;
    let before = probe::read(&probe::STATE_LOADS);
    let reply = store::workspace_references(&pool, &ws, "owner").await?;
    let loads = probe::read(&probe::STATE_LOADS) - before;
    println!("{}", json!({"state_loads":loads,"reply":reply}));
    ensure!(reply["workspaces"] == json!([]), "{reply}");
    ensure!(loads == 0, "the read loaded {loads} current states");
    // The owner check reads the owner from the meta row: a member is not the owner.
    let refused = store::workspace_references(&pool, &ws, "bot").await;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("owner")),
        "{refused:?}"
    );
    Ok(())
}

// ac:hqp77qd2
// test:kyoisdpc
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_lookup_of_the_naming_workspaces_reads_indexes_and_no_seq_scan() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let pool = pg_plan::db().await?;
    let target = pg_plan::workspace(&pool, "refs-target").await?;
    // Large tables: 4,000 objects and current rows in other workspaces. On a few rows a
    // sequential scan is the cheaper plan, and the criterion is about a real server.
    for _ in 0..2 {
        let other = pg_plan::workspace(&pool, "refs-other").await?;
        pg_plan::many_nodes(&pool, &other, 2000, 0).await?;
    }
    // One workspace names the target by a child workspace binding.
    let naming = pg_plan::workspace(&pool, "refs-naming").await?;
    let bind = json!({"op":"save_object","kind":"domain","id":"child","revision":0,"parent":null,
        "data":{"title":"Child","owner":"owner","workspace":target},"reason":"Bind"});
    store::execute_remote_input(&pool, &naming, "owner", "bind", &bind).await?;
    pg_plan::analyze(&pool).await?;
    let reply = store::workspace_references(&pool, &target, "owner").await?;
    ensure!(reply["workspaces"][0]["id"] == naming.as_str(), "{reply}");
    let sql = pg_plan::literal(SOURCE, "SELECT e.workspace FROM joppa.state_entries e")?;
    let conn = pool.get().await?;
    let plan = pg_plan::explain(&**conn, "FORMAT JSON", &sql, &[&target]).await?;
    let evidence = json!({"statement":sql,"plan":plan});
    println!("{evidence}");
    for table in ["objects", "state_entries"] {
        ensure!(
            pg_plan::seq_scans(&plan, table).is_empty(),
            "a Seq Scan on joppa.{table}: {evidence}"
        );
    }
    let indexes: Vec<String> = pg_plan::nodes(&plan)
        .iter()
        .filter_map(|n| n["Index Name"].as_str().map(str::to_owned))
        .collect();
    for index in ["objects_data_contains", "state_remote_links"] {
        ensure!(
            indexes.iter().any(|i| i == index),
            "{index} is not in the plan: {evidence}"
        );
    }
    Ok(())
}
