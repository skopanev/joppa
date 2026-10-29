mod common;
use common::*;
use joppa::{commands::Command, store};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
async fn execute(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    id: &str,
    v: Value,
) -> anyhow::Result<Value> {
    if is_removed(&v) {
        append_legacy(pool, ws, actor, id, &v).await?;
        return Ok(Value::Null);
    }
    store::execute(pool, ws, actor, id, &serde_json::from_value::<Command>(v)?).await
}
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL (database name must start with joppa_test)"]
async fn postgres_journal_concurrency_isolation_immutability_and_roundtrip() -> anyhow::Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let conn = pool.get().await?;
    let db: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        db.starts_with("joppa_test"),
        "refusing to run integration probes on a non-test database"
    );
    conn.batch_execute("CREATE TABLE IF NOT EXISTS public.legacy_probe(id integer PRIMARY KEY); INSERT INTO public.legacy_probe VALUES(1) ON CONFLICT DO NOTHING").await?;
    drop(conn);
    store::migrate(&pool).await?;
    store::migrate(&pool).await?;
    let ws = format!(
        "test-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    let before = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    assert_eq!(before["launch"]["ready"], false);
    assert!(
        store::read(&pool, &ws, "outsider", &store::Scope::full())
            .await
            .is_err()
    );
    let other = format!("{ws}-other");
    execute(
        &pool,
        &other,
        "outsider",
        "create",
        json!({"op":"workspace","title":"Another product"}),
    )
    .await?;
    execute(
        &pool,
        &other,
        "outsider",
        "same-local-name",
        json!({"op":"domain","id":"d","title":"Other domain","authority":"outsider"}),
    )
    .await?;
    assert_ne!(
        before["system_ids"]["domain"]["d"],
        store::read(&pool, &other, "outsider", &store::Scope::full()).await?["system_ids"]["domain"]
            ["d"]
    );
    let visible = store::list(&pool, "outsider").await?;
    assert!(visible.as_array().unwrap().iter().any(|w| w["id"] == other));
    assert!(!visible.as_array().unwrap().iter().any(|w| w["id"] == ws));
    assert!(
        execute(&pool, &other, "outsider", "foreign", assess("ios", true))
            .await
            .is_err()
    );
    // A live edit of requirement:r: the document of the old journal with a new title.
    let current = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("requirement:r"),
            ..Default::default()
        },
    )
    .await?;
    let edit = |revision: Value| {
        let mut data = current["document"]["data"].clone();
        data["title"] = json!("Paid access persists after sign-in");
        json!({"op":"save_object","kind":"requirement","id":"r","revision":revision,
            "parent":current["document"]["parent"],"data":data,"links":current["document"]["links"],"reason":"Agreed wording"})
    };
    let first = current["current_revision"].clone();
    let (a, b) = tokio::join!(
        execute(&pool, &ws, "agent", "edit-a", edit(first.clone())),
        execute(&pool, &ws, "agent", "edit-b", edit(first.clone()))
    );
    assert_ne!(a.is_ok(), b.is_ok(), "only one concurrent edit wins");
    let (winner, edit_result) = if a.is_ok() {
        ("edit-a", a?)
    } else {
        ("edit-b", b?)
    };
    let replay = execute(&pool, &ws, "agent", winner, edit(first.clone())).await?;
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["position"], edit_result["position"]);
    assert!(
        execute(&pool, &ws, "agent", winner, edit(json!(9)))
            .await
            .is_err()
    );
    execute(
        &pool,
        &ws,
        "owner",
        "confirm",
        json!({"op":"confirm","req":"r","revision":2}),
    )
    .await?;
    for ac in ["ios", "key"] {
        let mut v = assess(ac, true);
        v["revision"] = json!(2);
        execute(&pool, &ws, "agent", ac, v).await?;
    }
    execute(
        &pool,
        &ws,
        "owner",
        "scope",
        json!({"op":"define_verification_scope","req":"r","revision":2,"subject_keys":["app","backend","infra","environment"],"scope":"iOS access and provider settings","reason":"Full access and key scope"}),
    )
    .await?;
    let mut p = plan("run");
    p["revision"] = json!(2);
    execute(&pool, &ws, "runner", "plan", p).await?;
    let mut evidence = result("run", "[ios] paid access", "pass", vec!["access retained"]);
    evidence["revision"] = json!(2);
    let (a, b) = tokio::join!(
        execute(&pool, &ws, "runner", "result-a", evidence.clone()),
        execute(&pool, &ws, "runner", "result-b", evidence)
    );
    assert_ne!(a.is_ok(), b.is_ok(), "one immutable result wins");
    let mut evidence = result("run", "key-permissions", "pass", vec!["read", "verify"]);
    evidence["revision"] = json!(2);
    execute(&pool, &ws, "owner", "manual", evidence).await?;
    execute(
        &pool,
        &ws,
        "runner",
        "finish",
        json!({"op":"finish","req":"r","revision":2,"run":"run"}),
    )
    .await?;
    execute(
        &pool,
        &ws,
        "owner",
        "accept",
        json!({"op":"accept","req":"r","revision":2,"run":"run"}),
    )
    .await?;
    let card = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    assert_eq!(card["summary"]["r"]["accepted"], true);
    assert_eq!(card["launch"]["ready"], true);
    execute(&pool,&ws,"agent","bug-comment",json!({"op":"comment","req":"r","revision":2,"id":"bug","text":"Regression","kind":"comment","ac":"ios"})).await?;
    let (accept, issue) = tokio::join!(
        execute(
            &pool,
            &ws,
            "owner",
            "accept-race",
            json!({"op":"accept","req":"r","revision":2,"run":"run"})
        ),
        execute(
            &pool,
            &ws,
            "owner",
            "issue-race",
            json!({"op":"issue","req":"r","revision":2,"id":"bug","ac":"ios","comment":"bug"})
        )
    );
    issue?;
    let _ = accept;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?["summary"]["r"]["accepted"],
        false
    );
    let mut conn = pool.get().await?;
    assert_eq!(
        conn.query_one("SELECT count(*) FROM public.legacy_probe", &[])
            .await?
            .get::<_, i64>(0),
        1
    );
    for table in ["joppa.events", "joppa.workspaces", "joppa.item_ids"] {
        for sql in [
            format!(
                "UPDATE {table} SET {}={} WHERE false",
                if table.ends_with("events") {
                    "actor"
                } else {
                    "id"
                },
                if table.ends_with("events") {
                    "actor"
                } else {
                    "id"
                }
            ),
            format!("DELETE FROM {table} WHERE false"),
            format!("TRUNCATE {table} CASCADE"),
        ] {
            let tx = conn.transaction().await?;
            let error = tx.batch_execute(&sql).await.unwrap_err();
            assert!(
                error.as_db_error().unwrap().message().contains("WORM"),
                "{error:?}"
            );
            tx.rollback().await?;
        }
    }
    // Runtime role cannot disable triggers, rewrite history or own the schema.
    let tx = conn.transaction().await?;
    tx.batch_execute("CREATE ROLE joppa_test_runtime NOLOGIN; GRANT USAGE ON SCHEMA joppa TO joppa_test_runtime; GRANT SELECT,INSERT ON ALL TABLES IN SCHEMA joppa TO joppa_test_runtime; GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA joppa TO joppa_test_runtime; SET LOCAL ROLE joppa_test_runtime").await?;
    tx.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,coalesce(max(position),0)+1,'owner','runtime-create','{\"op\":\"domain\",\"id\":\"runtime\",\"title\":\"Runtime\",\"authority\":\"owner\"}'::jsonb FROM joppa.events WHERE workspace=$1",&[&ws]).await?;
    assert!(
        tx.query_opt(
            "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND local_id='runtime'",
            &[&ws]
        )
        .await?
        .is_some()
    );
    tx.batch_execute("SAVEPOINT permissions").await?;
    assert!(
        tx.batch_execute("ALTER TABLE joppa.events DISABLE TRIGGER events_worm")
            .await
            .is_err()
    );
    tx.batch_execute("ROLLBACK TO SAVEPOINT permissions")
        .await?;
    assert!(tx.batch_execute("DELETE FROM joppa.events").await.is_err());
    tx.rollback().await?;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?["state"]["requirements"]["r"]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        before["system_ids"],
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?["system_ids"]
    );
    Ok(())
}
