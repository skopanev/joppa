mod common;
use common::*;
use deadpool_postgres::Pool;
use joppa::store::{self, ObjectScope};
use serde_json::{Value, json};

async fn apply(pool: &Pool, ws: &str, actor: &str, command: Value) -> anyhow::Result<Value> {
    let request = uuid::Uuid::new_v4().to_string();
    if is_removed(&command) {
        append_legacy(pool, ws, actor, &request, &command).await?;
        return Ok(Value::Null);
    }
    store::execute_input(pool, ws, actor, &request, &command).await
}
async fn detail(pool: &Pool, ws: &str, key: &str) -> anyhow::Result<Value> {
    store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some(key),
            ..Default::default()
        },
    )
    .await
}
async fn action(
    pool: &Pool,
    ws: &str,
    actor: &str,
    key: &str,
    op: &str,
    extra: Value,
) -> anyhow::Result<()> {
    let obj = detail(pool, ws, key).await?;
    let mut command = json!({"op":op,"id":key,"revision":obj["current_revision"]});
    for (key, value) in extra.as_object().unwrap() {
        command[key] = value.clone();
    }
    apply(pool, ws, actor, command).await?;
    Ok(())
}
fn bound(run: &str, revision: u32, bindings: &[&str]) -> Value {
    let mut p = plan(run);
    p["revision"] = json!(revision);
    p["checks"][0]["tasks"] = json!(
        bindings
            .iter()
            .map(|address| json!({"address":address,"origin_revision":1}))
            .collect::<Vec<_>>()
    );
    p
}
async fn finish(
    pool: &Pool,
    ws: &str,
    run: &str,
    revision: u32,
    outcome: &str,
) -> anyhow::Result<()> {
    for (actor, check, outcome, observed) in [
        (
            "runner",
            "[ios] paid access",
            outcome,
            vec!["access retained"],
        ),
        ("owner", "key-permissions", "pass", vec!["read", "verify"]),
    ] {
        let mut command = result(run, check, outcome, observed);
        command["revision"] = json!(revision);
        apply(pool, ws, actor, command).await?;
    }
    apply(
        pool,
        ws,
        "runner",
        json!({"op":"finish","req":"r","revision":revision,"run":run}),
    )
    .await?;
    Ok(())
}

// [task-check-evidence-reporting]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn stored_bindings_update_current_signals_counters_and_coverage_without_replay()
-> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let db: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("task-evidence-{}", uuid::Uuid::new_v4().simple());
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["job"] = types["task"].clone();
    types["job"]["parents"] = json!(["*"]);
    types["section"] =
        json!({"title":"Section","role":"group","status":"children","parents":["*"],"root":true});
    apply(&pool,&ws,"owner",json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Use custom Task names through an intermediate group"})).await?;
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"section","id":"nested","revision":0,"parent":"ac:ios","data":{"title":"Nested work","owner":"owner"},"reason":"Place work at an arbitrary depth"})).await?;
    for id in ["one", "two"] {
        apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"job","id":id,"revision":0,"parent":"section:nested","data":{"title":id,"owner":"owner","contract":"Implement the fixture behavior","purpose":"implementation","projects":["app"]},"reason":"Create exact test contracts"})).await?;
    }
    for ac in ["ios", "key"] {
        apply(&pool, &ws, "agent", assess(ac, true)).await?;
    }
    let scope = |n| json!({"op":"define_verification_scope","req":"r","revision":n,"subject_keys":["app","backend","infra","environment"],"scope":"iOS access and provider settings","reason":"Define the isolated fixture subject"});
    apply(&pool, &ws, "owner", scope(1)).await?;
    conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
        .await?;
    conn.query_one("SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)", &[])
        .await?;
    let request = uuid::Uuid::new_v4().to_string();
    let command = bound("current", 1, &["job:one", "job:two"]);
    let first = store::execute_input(&pool, &ws, "runner", &request, &command).await?;
    let retry = store::execute_input(&pool, &ws, "runner", &request, &command).await?;
    assert_eq!(first["position"], retry["position"]);
    assert_eq!(retry["replayed"], true);
    finish(&pool, &ws, "current", 1, "pass").await?;
    for key in ["job:one", "job:two"] {
        assert!(
            detail(&pool, &ws, key).await?["process"]
                .get("verification")
                .is_none()
        );
    }
    let ios = detail(&pool, &ws, "ac:ios").await?;
    assert!(ios["process"].get("verification").is_none());
    action(&pool, &ws, "agent", "job:one", "claim_task", json!({})).await?;
    action(
        &pool,
        &ws,
        "agent",
        "job:one",
        "complete_task",
        json!({"evidence":"Fixture result delivered","commit":"c0ffee0"}),
    )
    .await?;
    action(
        &pool,
        &ws,
        "owner",
        "job:two",
        "retire_task",
        json!({"reason":"Fixture work no longer applies"}),
    )
    .await?;
    let candidates = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            ..Default::default()
        },
    )
    .await?;
    let ios = candidates["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "ios")
        .unwrap();
    assert_eq!(
        ios["work_summary"],
        json!({"active":0,"done":1,"retired":1})
    );
    let queue = store::task_queue(&pool, &ws, "agent", "all", 100, None).await?;
    let task = queue["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["address"] == "job:one")
        .unwrap();
    assert!(task["verification"].is_null());
    assert_eq!(task["origin_revision"], 1);
    assert_eq!(task["status"], "done");
    let journal:i64=conn.query_one(r#"SELECT COALESCE(SUM(calls),0)::bigint FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND query ~* '(FROM|JOIN)\s+joppa\.events'"#,&[]).await?.get(0);
    assert_eq!(
        journal, 0,
        "normal report and current read must not replay the journal"
    );

    apply(&pool, &ws, "agent", revision(1)).await?;
    let current = store::read(
        &pool,
        &ws,
        "agent",
        &store::Scope {
            full: false,
            req: Some("r".into()),
            revision: None,
            history: false,
            target: None,
        },
    )
    .await?;
    assert!(
        !current["requirement"]["revisions"][0]["prior_checks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for ac in ["ios", "key"] {
        let mut command = assess(ac, true);
        command["revision"] = json!(2);
        apply(&pool, &ws, "agent", command).await?;
    }
    action(
        &pool,
        &ws,
        "owner",
        "job:one",
        "review_task",
        json!({"reason":"Review the carried contract"}),
    )
    .await?;
    assert!(
        detail(&pool, &ws, "job:one").await?["process"]
            .get("verification")
            .is_none()
    );
    apply(&pool, &ws, "owner", scope(2)).await?;
    apply(&pool, &ws, "runner", bound("reduced", 2, &["job:one"])).await?;
    finish(&pool, &ws, "reduced", 2, "pass").await?;
    assert!(
        detail(&pool, &ws, "job:one").await?["process"]
            .get("verification")
            .is_none()
    );
    apply(&pool,&ws,"owner",json!({"op":"review_coverage","req":"r","revision":2,"run":"reduced","reason":"Review the retired Task coverage reduction"})).await?;
    assert!(
        detail(&pool, &ws, "job:one").await?["process"]
            .get("verification")
            .is_none()
    );
    apply(&pool, &ws, "runner", bound("newer", 2, &["job:one"])).await?;
    assert!(
        detail(&pool, &ws, "job:one").await?["process"]
            .get("verification")
            .is_none()
    );
    finish(&pool, &ws, "newer", 2, "fail").await?;
    let failed = detail(&pool, &ws, "job:one").await?;
    // A failing run keeps the Task done and shows no verification on it.
    assert!(failed["process"].get("verification").is_none());
    assert_eq!(failed["process"]["status"], "done");
    assert_eq!(failed["signal"]["tone"], "green");
    let failed_candidates = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            ..Default::default()
        },
    )
    .await?;
    let failed_ios = failed_candidates["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "ios")
        .unwrap();
    assert_eq!(
        failed_ios["work_summary"],
        json!({"active":0,"done":1,"retired":1})
    );
    println!(
        "Task evidence: shared=true, no_fake_confirmation=true, current_queue=true, journal_selects=0, stored_prior_checks=true, coverage_review=true, newer_pending=true, failure_keeps_done=true"
    );
    Ok(())
}
