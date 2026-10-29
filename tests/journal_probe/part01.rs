// Part 1 of tests/journal_probe.rs, included there, so the test names stay unchanged.

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
    common::seed(&pool, &ws, "owner", common::setup()).await?;
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
    let raw = "original source input ".repeat(46);
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
    save_title(
        &pool,
        &ws,
        "owner",
        "normal-write",
        "domain:d",
        "Measured current title",
        "Check current write access",
    )
    .await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "normal-create-domain",
        &json!({"op":"save_object","kind":"domain","id":"new","revision":0,"parent":null,"data":{"title":"Measured new domain","owner":"owner"},"reason":"Setup"}),
    )
    .await?;
    store::execute_input(&pool, &ws, "agent", "normal-create-input", &json!({"op":"save_object","kind":"note","id":"new-input","revision":0,"parent":null,"data":{"title":"Measured new Input","owner":"owner","body":"New input"},"links":[],"reason":"Check new object registration"})).await?;
    store::execute_input(&pool, &ws, "owner", "normal-create-capability", &json!({"op":"save_object","kind":"capability","id":"new-capability","revision":0,"parent":"domain:d","data":{"title":"Measured new capability","owner":"owner"},"reason":"Setup"})).await?;
    store::execute_input(&pool, &ws, "owner", "normal-create-consumer", &json!({"op":"save_object","kind":"consumer","id":"new-consumer","revision":0,"parent":null,"data":{"title":"Measured new consumer","owner":"owner"},"reason":"Setup"})).await?;
    store::execute_input(&pool, &ws, "owner", "normal-create-requirement", &json!({"op":"save_object","kind":"requirement","id":"new","revision":0,"parent":"capability:c","data":{"title":"Measured new requirement","owner":"owner","body":"New rule"},"reason":"Setup"})).await?;
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
    // Old journals hold revise; the replay reads it.
    common::append_legacy(&pool, &ws, "agent", "revision-2", &rev).await?;
    store::execute_input(&pool, &ws, "legal", "decision", &json!({"op":"comment","req":"r","revision":2,"id":"law","text":"Confirmed rights","kind":"decision","ac":"key"})).await?;
    rev["expected_revision"] = json!(2);
    rev["acs"][0]["text"] = json!("On iOS access survives reauthentication");
    common::append_legacy(&pool, &ws, "agent", "revision-3", &rev).await?;
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
