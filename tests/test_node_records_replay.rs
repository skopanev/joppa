//! requirement:test-nodes: a journal of an older server replays with the same tones. Its schema
//! revision has the removed name rule `test_results.name`, and its records name each test by
//! its title. A live write after it, a rename included, equals a full replay of the journal.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

const OVER: &str = "tests/refund.rs > over_limit";
const UNDER: &str = "tests/refund.rs > under_limit";
const SHARED: &str = "tests/refund.rs > shared";
const OLD_RULE: &str =
    r#"{"separator":" > ","min_parts":2,"file_extension":true,"max_length":400}"#;

async fn pool() -> Result<deadpool_postgres::Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let database: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok(pool.clone())
}

async fn write(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    command: Value,
) -> Result<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, actor, &id, &command).await
}

fn node(id: &str, parent: &str, title: &str) -> Value {
    json!({"op":"save_object","kind":"test","id":id,"revision":0,"parent":parent,
        "data":{"title":title,"owner":"owner"},"reason":"Bind"})
}

/// The stored test status and the tone of each present object.
async fn tones(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    let sql = "SELECT jsonb_object_agg(address,jsonb_build_array(test_status,COALESCE(rollup->'signal'->>'tone',signal->>'tone')))::text FROM joppa.objects WHERE workspace=$1 AND present";
    let text: String = pool.get().await?.query_one(sql, &[&ws]).await?.get(0);
    Ok(serde_json::from_str(&text)?)
}

fn tone(all: &Value, address: &str) -> String {
    all[address][1].as_str().unwrap_or_default().to_owned()
}

/// Rebuild the state and every projection of the workspace from its journal.
async fn replay(pool: &deadpool_postgres::Pool, ws: &str) -> Result<()> {
    let conn = pool.get().await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::migrate(pool).await?;
    store::backfill_board(pool).await?;
    Ok(())
}

/// The older server: its schema revision and its set_tree_schema event have the name rule, and
/// each of its records stored the results by title with a record_report event.
async fn older_journal(pool: &deadpool_postgres::Pool, ws: &str) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.batch_execute("SET LOCAL session_replication_role = replica")
        .await?;
    let rule: Value = serde_json::from_str(OLD_RULE)?;
    tx.execute("UPDATE joppa.events SET command=jsonb_set(command,'{test_results,name}',$2::text::jsonb) WHERE workspace=$1 AND command->>'op'='set_tree_schema' AND command ? 'test_results'", &[&ws, &rule.to_string()]).await?;
    tx.execute("UPDATE joppa.state_revisions SET value=jsonb_set(value,'{test_results,name}',$2::text::jsonb) WHERE workspace=$1 AND section='tree_schema' AND value ? 'test_results'", &[&ws, &rule.to_string()]).await?;
    for (commit, results) in [
        (
            "a".repeat(40),
            json!([[OVER, "pass"], [UNDER, "pass"], [SHARED, "pass"]]),
        ),
        ("b".repeat(40), json!([[UNDER, "fail"]])),
    ] {
        let report: i64 = tx.query_one("INSERT INTO joppa.test_reports(workspace,commit_sha,commit_time,commit_order,branch,default_branch,dirty,full_run,uploader,stamp_hash,junit,recorded) SELECT $1,$2,NULL,COALESCE(MAX(commit_order),0)+1,'record',true,false,false,'bot',md5($2),'',true FROM joppa.test_reports WHERE workspace=$1 RETURNING id", &[&ws, &commit]).await?.get(0);
        let rows = results.as_array().unwrap();
        for row in rows {
            let (test, status) = (row[0].as_str().unwrap(), row[1].as_str().unwrap());
            tx.execute("INSERT INTO joppa.test_results(report_id,address,status,status_name) VALUES($1,$2,$3,$3)", &[&report, &test, &status]).await?;
        }
        let event = json!({"op":"record_report","report":report,"commit":commit,"branch":"record","tests":rows.len()});
        tx.execute("INSERT INTO joppa.events(workspace,position,request_id,actor,command) SELECT $1,max(position)+1,$2,'bot',$3::text::jsonb FROM joppa.events WHERE workspace=$1", &[&ws, &uuid::Uuid::new_v4().to_string(), &event.to_string()]).await?;
    }
    tx.commit().await?;
    Ok(())
}

// ac:vtr2e65t
// test:4495ikum
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_older_journal_with_the_name_rule_and_title_records_replays_with_the_same_tones()
-> Result<()> {
    let pool = pool().await?;
    let ws = format!("older-{}", uuid::Uuid::new_v4().simple());
    let create = json!({"op":"workspace","title":"Older journal"});
    store::execute_input(&pool, &ws, "owner", "workspace", &create).await?;
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    write(&pool, &ws, "owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","root":true,"parents":[],"status":"children"},
        "ac":{"title":"Criteria","role":"ac","parents":["requirement"],"status":"children"},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400}
    },"test_results":tree["schema"]["test_results"]})).await?;
    for command in [
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"owner","body":"Refunds"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r","data":{"title":"B","owner":"owner"},"reason":"Create"}),
        node("over", "ac:a", OVER),
        node("under", "ac:a", UNDER),
        node("shared-a", "ac:a", SHARED),
        node("shared-b", "ac:b", SHARED),
    ] {
        write(&pool, &ws, "bot", command).await?;
    }
    older_journal(&pool, &ws).await?;
    // The full replay reads the older schema and binds each title record as the older server
    // did: a title binds every present test node with it.
    replay(&pool, &ws).await?;
    let replayed = tones(&pool, &ws).await?;
    let expected = [
        ("test:over", "green"),
        ("test:under", "red"),
        ("test:shared-a", "green"),
        ("test:shared-b", "green"),
        ("ac:a", "red"),
        ("ac:b", "green"),
    ];
    for (address, wanted) in expected {
        ensure!(tone(&replayed, address) == wanted, "{address}: {replayed}");
    }
    // The bot saves a business title, and the owner renames an older test node. The renamed
    // node keeps its title records and its tone.
    write(
        &pool,
        &ws,
        "bot",
        node("refund", "ac:b", "A refund over the limit is refused"),
    )
    .await?;
    let rename = json!({"op":"save_object","kind":"test","id":"over","revision":1,"parent":"ac:a",
        "data":{"title":"A refund of 50 EUR over the limit of 40 EUR is refused","owner":"owner"},"reason":"Business title"});
    write(&pool, &ws, "owner", rename).await?;
    let live = tones(&pool, &ws).await?;
    ensure!(tone(&live, "test:over") == "green", "{live}");
    ensure!(
        tone(&live, "test:refund") == "grey" && tone(&live, "ac:b") == "grey",
        "{live}"
    );
    // A full replay of the journal gives the same statuses as the live writes.
    replay(&pool, &ws).await?;
    let again = tones(&pool, &ws).await?;
    ensure!(again == live, "live {live}\nreplay {again}");
    // A live schema with the name rule is refused with its replacement.
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut block = tree["schema"]["test_results"].clone();
    block["name"] = serde_json::from_str(OLD_RULE)?;
    let refused = write(
        &pool,
        &ws,
        "owner",
        json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "reason":"The old name rule","test_results":block}),
    )
    .await;
    let error = refused.err().map(|e| e.to_string()).unwrap_or_default();
    ensure!(
        error.contains(joppa::removed_ops::TEST_NAME) && error.contains("test_results.name"),
        "{error}"
    );
    Ok(())
}
