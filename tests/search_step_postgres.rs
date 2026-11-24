//! Black-box checks of chunked full search rebuilds and enrollment queue markers
//! (ac:8nfurwt5 and ac:5lrja53z).
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::collections::BTreeMap;

async fn setup_domain_schema(w: &record_world::World) -> Result<()> {
    let tree = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = tree["schema"]["types"].clone();
    types["domain"] =
        json!({"title":"Domains","role":"group","status":"none","root":true,"parents":[]});
    store::execute_remote_input(
        &w.pool,
        &w.ws,
        "owner",
        "schema-domain",
        &json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":types,"reason":"Add domain"}),
    )
    .await?;
    Ok(())
}

// ac:8nfurwt5
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn full_search_build_writes_rows_in_steps_of_500_objects_and_deletes_by_kind_and_id()
-> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    setup_domain_schema(&w).await?;

    // Save 1,201 present domains in batches of 100 and one archived domain.
    for chunk in 0..13 {
        let start = chunk * 100;
        let end = ((chunk + 1) * 100).min(1201);
        let ops: Vec<Value> = (start..end)
            .map(|i| {
                json!({"command":{
                    "op":"save_object",
                    "kind":"domain",
                    "id":format!("d{i:04}"),
                    "revision":0,
                    "parent":null,
                    "data":{"title":format!("Domain {i:04}"),"owner":"owner","body":format!("Body text {i:04}")},
                    "reason":"Seed"
                }})
            })
            .collect();
        w.mcp(
            &w.bot,
            "joppa_batch",
            json!({"request_id":format!("seed-{chunk}"),"operations":ops}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    }
    w.mcp(
        &w.bot,
        "joppa_command",
        json!({"request_id":"seed-arch","command":{"op":"save_object","kind":"domain","id":"darch","revision":0,"parent":null,"data":{"title":"Archived domain","owner":"owner","body":"Old"},"reason":"Seed"}}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // First build each domain row through normal pending drain so we have the single-object
    // reference rows, then archive darch and leave a stale row + "*" in search_pending.
    w.mcp(&w.bot, "joppa_search", json!({"query":"Domain 0000"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let conn = w.pool.get().await?;
    let expected: BTreeMap<String, (String, String)> = conn
        .query(
            "SELECT local_id, title, content FROM joppa.search_documents WHERE workspace=$1 AND kind='domain' AND local_id<>'darch'",
            &[&w.ws],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), (r.get(1), r.get(2))))
        .collect();
    ensure!(
        expected.len() == 1201,
        "expected 1201, got {}",
        expected.len()
    );

    w.mcp(
        &w.bot,
        "joppa_command",
        json!({"request_id":"arch-it","command":{"op":"archive_object","target":"domain:darch","revision":1,"reason":"Archive"}}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    // Put back a stale search_documents row for darch and replace search_pending with "*".
    conn.execute(
        "INSERT INTO joppa.search_documents(workspace,kind,local_id,revision,title,content,content_hash) VALUES($1,'domain','darch',1,'Stale','Stale','00') ON CONFLICT DO NOTHING",
        &[&w.ws],
    )
    .await?;
    conn.execute(
        "DELETE FROM joppa.search_pending WHERE workspace=$1",
        &[&w.ws],
    )
    .await?;
    let max_pos: i64 = conn
        .query_one(
            "SELECT max(position) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    conn.execute(
        "INSERT INTO joppa.search_pending(workspace,address,position) VALUES($1,'*',$2)",
        &[&w.ws, &max_pos],
    )
    .await?;
    conn.batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .await?;
    // Wait for background indexer quiet, then reset pg_stat_statements right before joppa_search.
    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
    // Re-ensure "*" is waiting in case the background indexer drained during sleep.
    let still: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.search_pending WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    if still == 0 {
        conn.execute(
            "INSERT INTO joppa.search_documents(workspace,kind,local_id,revision,title,content,content_hash) VALUES($1,'domain','darch',1,'Stale','Stale','00') ON CONFLICT DO NOTHING",
            &[&w.ws],
        )
        .await?;
        conn.execute(
            "INSERT INTO joppa.search_pending(workspace,address,position) VALUES($1,'*',$2)",
            &[&w.ws, &max_pos],
        )
        .await?;
    }
    conn.query_one(
        "SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)",
        &[],
    )
    .await?;

    let hit = w
        .mcp(&w.bot, "joppa_search", json!({"query":"Domain 1200"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        hit["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|i| i["address"] == "domain:d1200")),
        "{hit}"
    );

    let stmts = conn
        .query(
            "SELECT lower(query), calls FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database())",
            &[],
        )
        .await?;
    let mut inserts = 0i64;
    let mut unnest_deletes = 0i64;
    let mut jsonb_deletes = 0i64;
    for row in &stmts {
        let (q, calls): (String, i64) = (row.get(0), row.get(1));
        if q.contains("insert into joppa.search_documents") {
            inserts += calls;
        }
        if q.contains("delete from joppa.search_documents") {
            if q.contains("unnest(") && !q.contains("jsonb_to_recordset") {
                unnest_deletes += calls;
            }
            if q.contains("jsonb_to_recordset") {
                jsonb_deletes += calls;
            }
        }
    }
    // 1,201 domains + requirement:r + ac:a + test = 1,204 objects -> ceil(1204 / 500) = 3 steps.
    ensure!(inserts == 3, "expected 3 INSERT steps, got {inserts}");
    ensure!(
        unnest_deletes == 1 && jsonb_deletes == 0,
        "unexpected DELETE statements: unnest={unnest_deletes}, jsonb={jsonb_deletes}"
    );

    let actual: BTreeMap<String, (String, String)> = conn
        .query(
            "SELECT local_id, title, content FROM joppa.search_documents WHERE workspace=$1 AND kind='domain'",
            &[&w.ws],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), (r.get(1), r.get(2))))
        .collect();
    ensure!(
        !actual.contains_key("darch"),
        "archived domain stayed in search_documents"
    );
    ensure!(
        actual == expected,
        "chunked build rows differ from single-domain build"
    );
    Ok(())
}

include!("search_step_postgres/part01.rs");
include!("search_step_postgres/part02.rs");
