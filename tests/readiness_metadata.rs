mod common;
use common::*;
use joppa::{objects, store, tasks};
use serde_json::{Value, json};

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok(
        "owner",
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "parent":parent,"data":data,"reason":"Set the current contract"}),
    );
}
fn work(title: &str, purpose: &str) -> Value {
    json!({"title":title,"owner":"legal","contract":"Implement the specified behavior",
        "purpose":purpose,"projects":[]})
}
fn native() -> World {
    let mut w = World::new();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    save(
        &mut w,
        "task:work",
        Some("ac:ios"),
        work("Implement access", "implementation"),
    );
    w
}

// [open-questions-visible-ac2]
#[test]
fn current_assessment_and_claim_refusal_show_the_same_question() {
    let mut w = native();
    assert_eq!(
        objects::process(&w.state, "ac:ios")["assessment"]["state"],
        "unassessed"
    );
    let unassessed = tasks::claimable(
        &w.state,
        "agent",
        "task:work",
        &w.state.task_process["task:work"],
    )
    .unwrap_err()
    .to_string();
    assert!(unassessed.contains("no readiness assessment"));
    w.ok("agent", assess("ios", false));
    let ac = objects::process(&w.state, "ac:ios");
    let task = objects::process(&w.state, "task:work");
    assert_eq!(ac["assessment"]["state"], "waiting");
    assert_eq!(ac["assessment"]["recipient"], "legal");
    assert_eq!(ac["assessment"], task["assessment"]);
    assert_eq!(
        objects::signal(&w.state, "task:work")["readiness_blocker"],
        ac["assessment"]
    );
    assert_eq!(objects::signal(&w.state, "task:work")["tone"], "yellow");
    let refusal = tasks::claimable(
        &w.state,
        "agent",
        "task:work",
        &w.state.task_process["task:work"],
    )
    .unwrap_err()
    .to_string();
    assert!(refusal.contains("Which exact permissions?"));
    assert!(refusal.contains("Recipient: legal"));
    w.ok("agent", assess("ios", true));
    assert_eq!(
        objects::process(&w.state, "task:work")["assessment"]["state"],
        "ready"
    );
    assert!(objects::process(&w.state, "task:work")["readiness_blocker"].is_null());
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:work",
            &w.state.task_process["task:work"]
        )
        .is_ok()
    );
    assert_eq!(
        objects::process(&w.state, "ac:ios")["verification"],
        "unverified"
    );
    let signal = objects::signal(&w.state, "ac:ios");
    assert_eq!(signal["tone"], "grey");
    assert_eq!(signal["label"], "Ready for work");
}

#[test]
fn revision_reset_removes_the_old_question_without_removing_history() {
    let mut w = native();
    w.ok("agent", assess("ios", false));
    let before = objects::process(&w.state, "ac:ios")["assessment"].clone();
    w.ok("owner", revision(1));
    let current = objects::process(&w.state, "ac:ios");
    assert_eq!(current["assessment"]["state"], "unassessed");
    assert_eq!(current["assessment"]["revision"], 2);
    assert!(current["assessment"]["reason"].is_null());
    assert!(current["assessment"]["recipient"].is_null());
    assert_ne!(current["assessment"], before);
    assert_eq!(
        w.state.requirements["r"].revisions[0].acs[0]
            .assessment
            .as_ref()
            .unwrap()
            .reason,
        "Which exact permissions?"
    );
    let task = objects::process(&w.state, "task:work");
    assert_eq!(task["assessment"], current["assessment"]);
    assert_eq!(task["review_required"], true);
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:work",
            &w.state.task_process["task:work"]
        )
        .unwrap_err()
        .to_string()
        .contains("owner review")
    );
}

#[test]
fn dependency_question_blocks_work_but_own_decision_task_remains_available() {
    let mut w = World::new();
    let mut next = revision(1);
    next["acs"][0]["depends_on"] = json!(["key"]);
    w.ok("owner", next);
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    for (ac, ready) in [("ios", true), ("key", false)] {
        let mut cmd = assess(ac, ready);
        cmd["revision"] = json!(2);
        w.ok("agent", cmd);
    }
    save(
        &mut w,
        "task:work",
        Some("ac:ios"),
        work("Implement access", "implementation"),
    );
    let p = objects::process(&w.state, "task:work");
    assert_eq!(p["assessment"]["state"], "ready");
    assert_eq!(p["readiness_blocker"]["criterion"], "key");
    assert_eq!(p["readiness_blocker"]["recipient"], "legal");
    save(
        &mut w,
        "task:dependent-decision",
        Some("ac:ios"),
        work("Collect a dependent decision", "decision"),
    );
    let dependent = objects::process(&w.state, "task:dependent-decision");
    assert_eq!(dependent["readiness_blocker"]["criterion"], "key");
    let refusal = tasks::claimable(
        &w.state,
        "legal",
        "task:dependent-decision",
        &w.state.task_process["task:dependent-decision"],
    )
    .unwrap_err()
    .to_string();
    assert!(refusal.contains("Which exact permissions?"));
    assert!(refusal.contains("Recipient: legal"));
    save(
        &mut w,
        "task:decision",
        Some("ac:key"),
        work("Collect the decision", "decision"),
    );
    assert_eq!(
        objects::process(&w.state, "task:decision")["assessment"]["state"],
        "waiting"
    );
    assert!(objects::process(&w.state, "task:decision")["readiness_blocker"].is_null());
    assert!(
        tasks::claimable(
            &w.state,
            "legal",
            "task:decision",
            &w.state.task_process["task:decision"]
        )
        .is_ok()
    );
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:decision",
            &w.state.task_process["task:decision"]
        )
        .is_err()
    );
}

#[test]
fn custom_role_names_and_parent_changes_select_the_current_assessment() {
    let mut w = World {
        state: Default::default(),
        position: 0,
    };
    w.ok("owner", json!({"op":"workspace","title":"Custom roles"}));
    w.ok("owner", json!({"op":"member","id":"legal","role":"human"}));
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
    ] {
        let cmd = serde_json::from_value(json!({"op":op})).unwrap();
        joppa::commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
        w.position += 1;
    }
    assert!(w.state.schema_migrated);
    assert!(w.state.documents_migrated);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":{
        "spec":{"title":"Specification","role":"requirement","root":true},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]},
        "work":{"title":"Work","role":"task","parents":["*"],"parent_gate":"ready"}
    },"reason":"Configure semantic roles"}),
    );
    for id in ["a", "b"] {
        save(
            &mut w,
            &format!("spec:{id}"),
            None,
            json!({"title":id,"owner":"owner","body":"Current contract"}),
        );
        save(
            &mut w,
            &format!("criterion:{id}"),
            Some(&format!("spec:{id}")),
            json!({"title":"Specified behavior","owner":"owner"}),
        );
    }
    let (req, r, ac) = objects::criterion_context(&w.state, "criterion:a").unwrap();
    let cmd = json!({"op":"assess","req":req,"revision":r.number,"ac":ac.id,"assessment":{
        "ready":false,"reason":"Which account can use this?","recipient":"legal","decision":null}});
    w.ok("owner", cmd);
    save(
        &mut w,
        "work:one",
        Some("criterion:a"),
        work("Implement behavior", "implementation"),
    );
    let p = objects::process(&w.state, "work:one");
    assert_eq!(p["assessment"]["state"], "waiting");
    assert_eq!(p["assessment"]["req"], "spec:a");
    assert!(
        tasks::claimable(
            &w.state,
            "legal",
            "work:one",
            &w.state.task_process["work:one"]
        )
        .unwrap_err()
        .to_string()
        .contains("Which account")
    );
    save(
        &mut w,
        "work:one",
        Some("criterion:b"),
        work("Implement behavior", "implementation"),
    );
    let p = objects::process(&w.state, "work:one");
    assert_eq!(p["assessment"]["state"], "unassessed");
    assert_eq!(p["assessment"]["req"], "spec:b");
    assert!(p["assessment"]["reason"].is_null());
    assert_eq!(p["review_required"], true);
}

#[test]
fn readiness_metadata_preserves_passing_and_failing_check_evidence() {
    for outcome in ["pass", "fail"] {
        let mut w = World::new();
        w.ready();
        w.run("actual-checks", outcome);
        let before = serde_json::to_value(&w.state).unwrap();
        assert_eq!(
            objects::process(&w.state, "ac:ios")["assessment"]["state"],
            "ready"
        );
        assert_eq!(
            objects::process(&w.state, "ac:ios")["verification"],
            outcome
        );
        assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
    }
}

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
    for (n, cmd) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{n}"), &cmd).await?;
    }
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
    store::execute_input(&pool, &ws, "owner", "revision-reset", &revision(1)).await?;
    let reset = store::task_queue(&runtime, &ws, "agent", "waiting", 100, None).await?;
    assert_eq!(reset["tasks"][0]["assessment"]["state"], "unassessed");
    assert!(reset["tasks"][0]["assessment"]["reason"].is_null());
    assert_eq!(reset["tasks"][0]["review_required"], true);
    Ok(())
}
