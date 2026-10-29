// Part 3 of tests/readiness_metadata.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn refresh_is_current_only_idempotent_and_preserves_status_time_and_history()
-> anyhow::Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("readiness-{}", uuid::Uuid::new_v4().simple());
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    store::execute_input(&pool, &ws, "agent", "waiting", &assess("ios", false)).await?;
    store::execute_input(&pool, &ws, "agent", "work", &json!({"op":"save_object","kind":"task","id":"one","revision":0,
        "parent":"ac:ios","data":work("Implement access", "implementation"),"reason":"Create work"})).await?;
    let conn = pool.get().await?;
    let history: String = conn.query_one("SELECT jsonb_agg(to_jsonb(r) ORDER BY address,revision)::text FROM joppa.object_revisions r WHERE workspace=$1", &[&ws]).await?.get(0);
    let position: i64 = conn
        .query_one(
            "SELECT position FROM joppa.board_cache WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    conn.execute("UPDATE joppa.board_cache SET first_view=first_view-'readiness_metadata_version' WHERE workspace=$1", &[&ws]).await?;
    conn.execute("UPDATE joppa.objects SET process=process-'assessment'-'readiness_blocker',signal=(signal-'assessment'-'readiness_blocker')||'{\"at\":\"2026-01-01T00:00:00Z\"}'::jsonb WHERE workspace=$1", &[&ws]).await?;
    conn.execute(
        "UPDATE joppa.task_queue SET data=data-'assessment'-'readiness_blocker' WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    let role = format!("readiness_runtime_{}", uuid::Uuid::new_v4().simple());
    conn.batch_execute(&format!("CREATE ROLE {role} NOLOGIN; GRANT USAGE ON SCHEMA joppa TO {role}; GRANT SELECT ON ALL TABLES IN SCHEMA joppa TO {role}; REVOKE SELECT ON joppa.events,joppa.object_revisions,joppa.state_revisions FROM {role}; GRANT INSERT,UPDATE ON joppa.objects,joppa.object_aliases,joppa.task_queue TO {role}; GRANT UPDATE ON joppa.board_cache TO {role}; GRANT DELETE ON joppa.task_queue TO {role};")).await?;
    drop(conn);
    let mut runtime_url = reqwest::Url::parse(&url)?;
    runtime_url
        .query_pairs_mut()
        .append_pair("options", &format!("-c role={role}"));
    let runtime = store::pool(
        &runtime_url
            .as_str()
            .replace("options=-c+role", "options=-c%20role"),
    )?;
    assert!(
        runtime
            .get()
            .await?
            .query("SELECT position FROM joppa.events", &[])
            .await
            .is_err()
    );
    store::refresh_readiness_metadata(&runtime).await?;
    let read = store::read_objects(
        &runtime,
        &ws,
        "agent",
        &store::ObjectScope {
            address: Some("ac:ios"),
            limit: 100,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(read["process"]["assessment"]["state"], "waiting");
    assert_eq!(read["process"]["assessment"]["recipient"], "legal");
    assert_eq!(read["signal"]["at"], "2026-01-01T00:00:00Z");
    let queue = store::task_queue(&runtime, &ws, "agent", "waiting", 100, None).await?;
    assert_eq!(
        queue["tasks"][0]["assessment"],
        read["process"]["assessment"]
    );
    assert!(
        queue["tasks"][0]["waiting_reason"]
            .as_str()
            .unwrap()
            .contains("Which exact permissions?")
    );
    let conn = pool.get().await?;
    let versions: Vec<String> = conn
        .query(
            "SELECT address,xmin::text FROM joppa.objects WHERE workspace=$1 ORDER BY address",
            &[&ws],
        )
        .await?
        .into_iter()
        .map(|r| r.get(1))
        .collect();
    store::refresh_readiness_metadata(&runtime).await?;
    let after: Vec<String> = conn
        .query(
            "SELECT address,xmin::text FROM joppa.objects WHERE workspace=$1 ORDER BY address",
            &[&ws],
        )
        .await?
        .into_iter()
        .map(|r| r.get(1))
        .collect();
    assert_eq!(versions, after, "second refresh must not write objects");
    assert_eq!(
        position,
        conn.query_one(
            "SELECT position FROM joppa.board_cache WHERE workspace=$1",
            &[&ws]
        )
        .await?
        .get::<_, i64>(0)
    );
    assert_eq!(history, conn.query_one("SELECT jsonb_agg(to_jsonb(r) ORDER BY address,revision)::text FROM joppa.object_revisions r WHERE workspace=$1", &[&ws]).await?.get::<_,String>(0));
    drop(conn);
    let mut changed = assess("ios", false);
    changed["assessment"]["reason"] = json!("Which tenant receives access?");
    changed["assessment"]["recipient"] = json!("owner");
    store::execute_input(&pool, &ws, "agent", "question-update", &changed).await?;
    let waiting = store::task_queue(&runtime, &ws, "agent", "waiting", 100, None).await?;
    assert_eq!(
        waiting["tasks"][0]["assessment"]["reason"],
        "Which tenant receives access?"
    );
    assert_eq!(waiting["tasks"][0]["assessment"]["recipient"], "owner");
    assert!(
        waiting["tasks"][0]["waiting_reason"]
            .as_str()
            .unwrap()
            .contains("Recipient: owner")
    );
    let current = store::read_objects(
        &runtime,
        &ws,
        "agent",
        &store::ObjectScope {
            address: Some("ac:ios"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        current["signal"]["assessment"],
        waiting["tasks"][0]["assessment"]
    );
    assert_ne!(current["signal"]["at"], "2026-01-01T00:00:00Z");
    store::execute_input(&pool, &ws, "agent", "ready", &assess("ios", true)).await?;
    let available = store::task_queue(&runtime, &ws, "agent", "available", 100, None).await?;
    assert_eq!(available["tasks"][0]["assessment"]["state"], "ready");
    append_legacy(&pool, &ws, "owner", "revision-reset", &revision(1)).await?;
    let reset = store::task_queue(&runtime, &ws, "agent", "waiting", 100, None).await?;
    assert_eq!(reset["tasks"][0]["assessment"]["state"], "unassessed");
    assert!(reset["tasks"][0]["assessment"]["reason"].is_null());
    assert_eq!(reset["tasks"][0]["review_required"], true);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn backfill_reprojects_work_ready_from_an_older_projection_version() -> anyhow::Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("readiness-{}", uuid::Uuid::new_v4().simple());
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    store::execute_input(&pool, &ws, "owner", "other", &json!({"op":"save_object","kind":"requirement","id":"other","revision":0,
        "parent":"capability:c","data":{"title":"Other scope","body":"Scope","owner":"owner","consumers":["customer"]},"reason":"Define"})).await?;
    let conn = pool.get().await?;
    let object = "SELECT revision,document::text,work_ready,process::text,signal::text FROM joppa.objects WHERE workspace=$1 AND address=$2";
    let (requirement, other) = (
        conn.query_one(object, &[&ws, &"requirement:r"]).await?,
        conn.query_one(object, &[&ws, &"requirement:other"]).await?,
    );
    let data = serde_json::from_str::<Value>(requirement.get(1))?["data"].clone();
    store::execute_input(&pool, &ws, "owner", "link", &json!({"op":"save_object","kind":"requirement","id":"r",
        "revision":requirement.get::<_,i32>(0),"parent":"capability:c","data":data,
        "links":[{"target":"requirement:other","relation":"depends_on","revision":other.get::<_,i32>(0),"blocking":true}],
        "reason":"Order after the other requirement"})).await?;
    let process: Value =
        serde_json::from_str(conn.query_one(object, &[&ws, &"ac:ios"]).await?.get(3))?;
    let mut ready = assess("ios", true);
    ready["revision"] = process["revision"].clone();
    store::execute_input(&pool, &ws, "agent", "ready", &ready).await?;
    let current = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    let full = (
        current.get::<_, bool>(2),
        serde_json::from_str::<Value>(current.get(3))?,
        serde_json::from_str::<Value>(current.get(4))?,
    );
    assert!(full.0);
    assert_eq!(full.1["assessment"]["state"], "ready");
    conn.execute("UPDATE joppa.objects SET work_ready=false,process=jsonb_set(process,'{assessment,state}','\"waiting\"') WHERE workspace=$1 AND address='ac:ios'", &[&ws]).await?;
    conn.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{projection_version}','\"8\"') WHERE workspace=$1", &[&ws]).await?;
    let stale = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    assert!(!stale.get::<_, bool>(2));
    store::backfill_board(&pool).await?;
    let repaired = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    assert!(repaired.get::<_, bool>(2));
    let repaired_process: Value = serde_json::from_str(repaired.get(3))?;
    assert_eq!(repaired_process["assessment"]["state"], "ready");
    assert_eq!(
        (
            repaired.get::<_, bool>(2),
            repaired_process,
            serde_json::from_str::<Value>(repaired.get(4))?
        ),
        full
    );
    let snapshot = "SELECT jsonb_build_array(b.xmin::text,b.first_view->>'projection_version',(SELECT jsonb_agg(jsonb_build_array(o.address,o.xmin::text) ORDER BY o.address) FROM joppa.objects o WHERE o.workspace=b.workspace))::text FROM joppa.board_cache b WHERE b.workspace=$1";
    let before: String = conn.query_one(snapshot, &[&ws]).await?.get(0);
    assert_eq!(serde_json::from_str::<Value>(&before)?[1], "15");
    store::backfill_board(&pool).await?;
    assert_eq!(
        conn.query_one(snapshot, &[&ws]).await?.get::<_, String>(0),
        before,
        "a current projection must not be rewritten"
    );
    Ok(())
}
