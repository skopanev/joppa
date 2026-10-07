mod common;
use common::*;
use deadpool_postgres::Pool;
use joppa::store;
use serde_json::{Value, json};
use std::collections::BTreeMap;

async fn reset(pool: &Pool) -> anyhow::Result<()> {
    pool.get().await?.query_one(
        "SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)",
        &[],
    ).await?;
    Ok(())
}

async fn selects(pool: &Pool, table: &str) -> anyhow::Result<Value> {
    let rows = pool.get().await?.query(
        "SELECT query,calls,rows FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND userid=(SELECT usesysid FROM pg_user WHERE usename=current_user) ORDER BY query",
        &[],
    ).await?;
    let statements: Vec<Value> = rows
        .into_iter()
        .filter_map(|r| {
            let query: String = r.get(0);
            let normalized = query
                .to_ascii_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (normalized.contains(&format!("from {table}"))
                || normalized.contains(&format!("join {table}")))
            .then(|| json!({"query":query,"calls":r.get::<_,i64>(1),"rows":r.get::<_,i64>(2)}))
        })
        .collect();
    let count: i64 = statements
        .iter()
        .map(|s| s["calls"].as_i64().unwrap())
        .sum();
    let rows: i64 = statements.iter().map(|s| s["rows"].as_i64().unwrap()).sum();
    Ok(json!({"calls":count,"rows":rows,"statements":statements}))
}

async fn journal_selects(pool: &Pool) -> anyhow::Result<Value> {
    selects(pool, "joppa.events").await
}

async fn fixture() -> anyhow::Result<(Pool, String)> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    pool.get()
        .await?
        .batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .await?;
    store::migrate(&pool).await?;
    let ws = format!("probe-{}", uuid::Uuid::new_v4().simple());
    for (n, cmd) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{n}"), &cmd).await?;
    }
    Ok((pool, ws))
}

async fn snapshot(pool: &Pool, workspace: &str) -> anyhow::Result<BTreeMap<String, Vec<String>>> {
    let conn = pool.get().await?;
    let mut out = BTreeMap::new();
    for table in [
        "objects",
        "object_revisions",
        "object_links",
        "ac_owners",
        "state_entries",
        "state_revisions",
        "task_queue",
        "search_documents",
    ] {
        let statement = format!("SELECT to_jsonb(t)::text FROM joppa.{table} t WHERE workspace=$1");
        let mut rows = vec![];
        for row in conn.query(&statement, &[&workspace]).await? {
            let mut value: Value = serde_json::from_str(row.get(0))?;
            let object = value.as_object_mut().unwrap();
            if table == "search_documents" {
                object.remove("terms");
                object.remove("embedding");
            }
            if let Some(signal) = object.get_mut("signal").and_then(Value::as_object_mut) {
                signal.remove("at");
            }
            rows.push(value.to_string());
        }
        rows.sort();
        out.insert(table.into(), rows);
    }
    Ok(out)
}

// [ac:17a50553-b267-4ecf-8f74-190486759f8d]
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn current_reads_and_writes_do_not_select_journal_events() -> anyhow::Result<()> {
    let (pool, ws) = fixture().await?;
    let raw = "original source input ".repeat(100_000);
    store::execute_input(&pool,&ws,"agent","input",&json!({"op":"save_object","kind":"note","id":"raw","revision":0,"parent":null,"data":{"title":"Source input","owner":"owner","body":raw,"status":"unprocessed"},"links":[],"reason":"Capture source"})).await?;
    let original_xmin: String = pool.get().await?.query_one("SELECT xmin::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key='note:raw'", &[&ws]).await?.get(0);
    reset(&pool).await?;
    store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            kind: Some("domain"),
            limit: 100,
            ..Default::default()
        },
    )
    .await?;
    store::read(
        &pool,
        &ws,
        "owner",
        &store::Scope::from_args(&json!({"req":"r"}))?,
    )
    .await?;
    store::task_queue(&pool, &ws, "owner", "available", 100, None).await?;
    store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            work_filter: Some("unstarted"),
            limit: 100,
            ..Default::default()
        },
    )
    .await?;
    let list = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            kind: Some("note"),
            parent: Some("*"),
            limit: 100,
            ..Default::default()
        },
    )
    .await?;
    assert!(
        !list.to_string().contains(&raw),
        "list exposed the unrelated input body"
    );
    let reads = journal_selects(&pool).await?;
    println!("Current read journal probe: {reads}");
    assert_eq!(reads["calls"], 0, "current reads selected journal events");

    reset(&pool).await?;
    store::execute_input(&pool, &ws, "owner", "normal-write", &json!({"op":"edit_domain","id":"d","title":"Measured current title","owner":"owner","reason":"Check current write access"})).await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "normal-create-domain",
        &json!({"op":"domain","id":"new","title":"Measured new domain","owner":"owner"}),
    )
    .await?;
    store::execute_input(&pool, &ws, "agent", "normal-create-input", &json!({"op":"save_object","kind":"note","id":"new-input","revision":0,"parent":null,"data":{"title":"Measured new Input","owner":"owner","body":"New input"},"links":[],"reason":"Check new object registration"})).await?;
    store::execute_input(&pool, &ws, "owner", "normal-create-capability", &json!({"op":"capability","id":"new-capability","title":"Measured new capability","domain":"d"})).await?;
    store::execute_input(&pool, &ws, "owner", "normal-create-consumer", &json!({"op":"consumer","id":"new-consumer","title":"Measured new consumer","owner":"owner"})).await?;
    let mut new_requirement = revision(0);
    new_requirement["req"] = json!("new");
    new_requirement["acs"][0]["id"] = json!("new-ios");
    new_requirement["acs"][1]["id"] = json!("new-key");
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "normal-create-requirement",
        &new_requirement,
    )
    .await?;
    let writes = journal_selects(&pool).await?;
    println!("Current write journal probe: {writes}");
    assert_eq!(writes["calls"], 0, "normal write selected journal events");
    let row = pool.get().await?.query_one("SELECT xmin::text,value->'revisions'->0->'data'->>'body',compact->'revisions'->0->'data' ? 'body' FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key='note:raw'", &[&ws]).await?;
    assert_eq!(row.get::<_, String>(0), original_xmin);
    assert_eq!(row.get::<_, String>(1), raw);
    assert!(
        !row.get::<_, bool>(2),
        "compact validation graph includes the input body"
    );
    println!(
        "Large input: bytes={}, unchanged_row=true, compact_body=false, list_body=false",
        raw.len()
    );

    let mut rev = revision(1);
    rev["acs"][1]["decider"] = json!("legal");
    store::execute_input(&pool, &ws, "agent", "revision-2", &rev).await?;
    store::execute_input(&pool, &ws, "legal", "decision", &json!({"op":"comment","req":"r","revision":2,"id":"law","text":"Confirmed rights","kind":"decision","ac":"key"})).await?;
    rev["expected_revision"] = json!(2);
    rev["acs"][0]["text"] = json!("On iOS access survives reauthentication");
    store::execute_input(&pool, &ws, "agent", "revision-3", &rev).await?;
    let mut assessment = assess("key", true);
    assessment["revision"] = json!(3);
    assessment["assessment"]["decision"] = json!("law");
    reset(&pool).await?;
    store::execute_input(&pool, &ws, "agent", "reuse-decision", &assessment).await?;
    let journal = journal_selects(&pool).await?;
    let history = selects(&pool, "joppa.state_revisions").await?;
    println!("Prior decision reuse: journal={journal}, history={history}");
    assert_eq!(journal["calls"], 0);
    assert_eq!(
        history["rows"], 1,
        "reuse must select only the decision revision"
    );

    let incremental = snapshot(&pool, &ws).await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    reset(&pool).await?;
    store::backfill_board(&pool).await?;
    let recovery = journal_selects(&pool).await?;
    println!("Administrative recovery journal probe: {recovery}");
    assert!(
        recovery["calls"].as_i64().unwrap() > 0,
        "probe must detect recovery journal reads"
    );
    assert_eq!(snapshot(&pool, &ws).await?, incremental);
    Ok(())
}

// [ac:17a50553-b267-4ecf-8f74-190486759f8d]
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn legacy_retry_projection_preserves_commands_and_batch_receipts() -> anyhow::Result<()> {
    let (pool, ws) = fixture().await?;
    let legacy = json!({"op":"domain","id":"legacy","title":"Legacy domain","authority":"owner"});
    let source = json!({"op":"link_source","req":"r","revision":1,"id":"uri-source","kind":"document","uri":"https://example.test/source","version":"1","relation":"origin"});
    let mut position: i64 = pool
        .get()
        .await?
        .query_one(
            "SELECT max(position) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    for (request, actor, command) in [
        ("legacy-domain", "owner", &legacy),
        ("legacy-uri", "agent", &source),
    ] {
        position += 1;
        pool.get().await?.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb)", &[&ws,&position,&actor,&request,&command.to_string()]).await?;
    }
    pool.get().await?.execute("UPDATE joppa.board_cache SET first_view=first_view-'legacy_receipts_version' WHERE workspace=$1", &[&ws]).await?;
    let before = snapshot(&pool, &ws).await?;
    reset(&pool).await?;
    let fallback = store::execute_input(&pool, &ws, "owner", "legacy-domain", &legacy).await?;
    assert_eq!(fallback["replayed"], true);
    assert_eq!(journal_selects(&pool).await?["calls"], 1);
    assert_eq!(
        snapshot(&pool, &ws).await?,
        before,
        "unmigrated retry must not repeat an effect"
    );

    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::backfill_board(&pool).await?;
    let before = snapshot(&pool, &ws).await?;
    let mut alias = legacy.clone();
    alias.as_object_mut().unwrap().remove("authority");
    alias["owner"] = json!("owner");
    reset(&pool).await?;
    for (request, actor, command) in [
        ("legacy-domain", "owner", &alias),
        ("legacy-uri", "agent", &source),
    ] {
        let retry = store::execute_input(&pool, &ws, actor, request, command).await?;
        assert_eq!(retry["replayed"], true);
    }
    assert_eq!(journal_selects(&pool).await?["calls"], 0);
    assert_eq!(snapshot(&pool, &ws).await?, before);
    assert!(
        store::execute_input(&pool, &ws, "agent", "legacy-domain", &alias)
            .await
            .is_err()
    );
    alias["title"] = json!("Different content");
    assert!(
        store::execute_input(&pool, &ws, "owner", "legacy-domain", &alias)
            .await
            .is_err()
    );
    let single = [store::BatchOperation {
        reference: None,
        command: legacy,
    }];
    assert!(
        store::execute_batch(&pool, &ws, "owner", "legacy-domain", &single)
            .await
            .is_err()
    );

    let operations = [store::BatchOperation {
        reference: Some("first".into()),
        command: json!({"op":"domain","id":"batch-group","title":"Batch group","owner":"owner"}),
    }];
    let batch = store::execute_batch(&pool, &ws, "owner", "outer-batch", &operations).await?;
    let batch_position = batch["results"][0]["position"].as_i64().unwrap();
    let private_request: String = pool
        .get()
        .await?
        .query_one(
            "SELECT request_id FROM joppa.events WHERE workspace=$1 AND position=$2",
            &[&ws, &batch_position],
        )
        .await?
        .get(0);
    reset(&pool).await?;
    let replay = store::execute_batch(&pool, &ws, "owner", "outer-batch", &operations).await?;
    assert_eq!(replay["position"], batch["position"]);
    assert_eq!(replay["replayed"], true);
    let changed = [store::BatchOperation {
        reference: Some("different".into()),
        command: operations[0].command.clone(),
    }];
    assert!(
        store::execute_batch(&pool, &ws, "owner", "outer-batch", &changed)
            .await
            .is_err()
    );
    assert_eq!(journal_selects(&pool).await?["calls"], 0);
    let receipt: String = pool.get().await?.query_one("SELECT input::text FROM joppa.command_receipts WHERE workspace=$1 AND request_id='outer-batch'", &[&ws]).await?.get(0);
    pool.get().await?.execute("UPDATE joppa.board_cache SET first_view=first_view-'legacy_receipts_version' WHERE workspace=$1", &[&ws]).await?;
    store::migrate(&pool).await?;
    store::migrate(&pool).await?;
    assert!(!pool.get().await?.query_one("SELECT EXISTS(SELECT 1 FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2)", &[&ws,&private_request]).await?.get::<_,bool>(0));
    assert!(
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &private_request,
            &operations[0].command
        )
        .await
        .is_err()
    );
    assert_eq!(pool.get().await?.query_one("SELECT input::text FROM joppa.command_receipts WHERE workspace=$1 AND request_id='outer-batch'", &[&ws]).await?.get::<_,String>(0), receipt);
    reset(&pool).await?;
    store::execute_input(&pool, &ws, "owner", "after-migration", &json!({"op":"edit_domain","id":"d","title":"After migration","owner":"owner","reason":"Check marker persistence"})).await?;
    store::execute_input(&pool, &ws, "owner", "after-projection", &json!({"op":"edit_domain","id":"d","title":"After projection","owner":"owner","reason":"Check marker persistence"})).await?;
    assert_eq!(journal_selects(&pool).await?["calls"], 0);
    let incremental = snapshot(&pool, &ws).await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::backfill_board(&pool).await?;
    assert_eq!(snapshot(&pool, &ws).await?, incremental);
    assert!(!pool.get().await?.query_one("SELECT EXISTS(SELECT 1 FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2)", &[&ws,&private_request]).await?.get::<_,bool>(0));
    assert!(
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &private_request,
            &operations[0].command
        )
        .await
        .is_err()
    );
    let replay = store::execute_batch(&pool, &ws, "owner", "outer-batch", &operations).await?;
    assert_eq!(replay["results"], batch["results"]);
    assert_eq!(replay["replayed"], true);
    println!(
        "Legacy retries: fallback_detected=true, migrated_journal_calls=0, alias=true, uri=true, actor_and_content_guard=true, complete_batch_receipt=true, marker_persists=true"
    );
    Ok(())
}

// [ac:17a50553-b267-4ecf-8f74-190486759f8d]
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn current_prefix_follows_batch_order_and_does_not_leave_the_transaction()
-> anyhow::Result<()> {
    for rename_first in [true, false] {
        let (pool, ws) = fixture().await?;
        pool.resize(1);
        let backend: i32 = pool
            .get()
            .await?
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let legacy_function: String = pool
            .get()
            .await?
            .query_one(
                "SELECT pg_get_functiondef('joppa.register_item(text,text,text)'::regprocedure)",
                &[],
            )
            .await?
            .get(0);
        let prefix = format!("prefix-{}", uuid::Uuid::new_v4().simple());
        let rename = store::BatchOperation {
            reference: None,
            command: json!({"op":"rename_workspace","title":"Renamed product","reference_prefix":prefix,"reason":"Check batch order"}),
        };
        let create = store::BatchOperation {
            reference: None,
            command: json!({"op":"domain","id":"ordered","title":"Ordered domain","owner":"owner"}),
        };
        let mut operations = if rename_first {
            vec![rename, create]
        } else {
            vec![create, rename]
        };
        let second_prefix = format!("second-{}", uuid::Uuid::new_v4().simple());
        operations.extend([
            store::BatchOperation { reference: None, command: json!({"op":"rename_workspace","title":"Renamed twice","reference_prefix":second_prefix,"reason":"Check the second batch rename"}) },
            store::BatchOperation { reference: None, command: json!({"op":"domain","id":"second","title":"Second domain","owner":"owner"}) },
        ]);
        let foreign = format!("foreign-{}", uuid::Uuid::new_v4().simple());
        // Administrative provisioning precedes the normal command probe.
        store::execute_input(
            &pool,
            &foreign,
            "owner",
            "workspace",
            &json!({"op":"workspace","title":"Foreign product"}),
        )
        .await?;
        reset(&pool).await?;
        let result =
            store::execute_batch(&pool, &ws, "owner", "ordered-batch", &operations).await?;
        let create_index = usize::from(rename_first);
        let expected_prefix = if rename_first { &prefix } else { &ws };
        assert!(
            result["results"][create_index]["system_ids"]["domain"]["ordered"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{expected_prefix}-D-"))
        );
        assert!(
            result["results"][3]["system_ids"]["domain"]["second"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{second_prefix}-D-"))
        );
        let other = store::execute_input(
            &pool,
            &foreign,
            "owner",
            "create",
            &json!({"op":"domain","id":"other","title":"Other domain","owner":"owner"}),
        )
        .await?;
        assert!(
            other["system_ids"]["domain"]["other"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{foreign}-D-"))
        );
        assert_eq!(journal_selects(&pool).await?["calls"], 0);
        reset(&pool).await?;
        assert!(
            store::execute_input(
                &pool,
                &ws,
                "owner",
                "invalid-create",
                &json!({"op":"domain","id":"invalid","title":"Invalid domain","owner":"unknown"})
            )
            .await
            .is_err()
        );
        let inserts: i64 = pool.get().await?.query_one("SELECT COALESCE(sum(calls),0)::bigint FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND userid=(SELECT usesysid FROM pg_user WHERE usename=current_user) AND query ILIKE $1", &[&"%INSERT INTO joppa.events%"]).await?.get(0);
        assert_eq!(
            inserts, 1,
            "rollback test must execute the context-setting event INSERT"
        );
        let row = pool
            .get()
            .await?
            .query_one(
                "SELECT pg_backend_pid(),NULLIF(current_setting('joppa.reference_prefix',true),'')",
                &[],
            )
            .await?;
        assert_eq!(row.get::<_, i32>(0), backend);
        assert!(
            row.get::<_, Option<String>>(1).is_none(),
            "prefix leaked after commit or rollback"
        );
        reset(&pool).await?;
        pool.get()
            .await?
            .execute(
                "SELECT joppa.register_item($1,'domain','admin-probe')",
                &[&ws],
            )
            .await?;
        let admin = journal_selects(&pool).await?;
        assert_eq!(
            admin["calls"], 1,
            "probe must detect the administrative legacy function"
        );
        assert_eq!(admin["rows"], 1);
        store::migrate(&pool).await?;
        assert_eq!(pool.get().await?.query_one("SELECT pg_get_functiondef('joppa.register_item(text,text,text)'::regprocedure)", &[]).await?.get::<_,String>(0), legacy_function);
        println!(
            "Current prefix: rename_first={rename_first}, journal_calls=0, same_backend=true, no_context_leak=true, legacy_admin_probe={admin}, legacy_function_preserved=true"
        );
    }
    Ok(())
}
