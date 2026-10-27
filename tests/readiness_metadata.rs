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
fn claim(w: &mut World, actor: &str, key: &str) -> anyhow::Result<()> {
    let revision = w.state.objects[key].current().revision;
    w.apply(
        actor,
        json!({"op":"claim_task","id":key,"revision":revision}),
    )
}
fn refuse_claim(w: &mut World, key: &str) -> String {
    let error = claim(w, "bot", key).unwrap_err().to_string();
    assert!(w.state.task_process[key].assignee.is_none());
    error
}

// [no-work-before-ready-ac1]
#[test]
fn tasks_under_not_ready_criteria_cannot_be_claimed() {
    let mut w = World::new();
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"bot","role":"agent"}),
    );
    w.ok("owner", json!({"op":"member","id":"alex","role":"human"}));
    let access = |expected: u32| {
        json!({"op":"revise","req":"access","expected_revision":expected,"capability":"c",
            "consumers":["customer"],"title":"Access","body":"Scope","owner":"owner",
            "reason":"Agreed wording","acs":[{"id":"a1","text":"First"},
            {"id":"a2","text":"Second"},{"id":"a3","text":"Third"}]})
    };
    w.ok("owner", access(0));
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    for (task, ac) in [
        ("task:t1", "ac:a1"),
        ("task:t2", "ac:a2"),
        ("task:t3", "ac:a3"),
    ] {
        save(
            &mut w,
            task,
            Some(ac),
            work("Implement access", "implementation"),
        );
    }
    w.ok(
        "agent",
        json!({"op":"assess","req":"access","revision":1,"ac":"a3","assessment":{
            "ready":true,"reason":"Known scope","recipient":null,"decision":null}}),
    );
    w.ok("owner", access(1));
    let assess_access = |ac: &str, ready: bool, reason: &str, recipient: Option<&str>| {
        json!({"op":"assess","req":"access","revision":2,"ac":ac,"assessment":{
            "ready":ready,"reason":reason,"recipient":recipient,"decision":null}})
    };
    w.ok(
        "agent",
        assess_access("a2", false, "Which permissions?", Some("alex")),
    );
    assert_eq!(
        refuse_claim(&mut w, "task:t1"),
        "Criterion a1 has no readiness assessment"
    );
    assert_eq!(
        refuse_claim(&mut w, "task:t2"),
        "Which permissions? · Recipient: alex"
    );
    assert_eq!(
        refuse_claim(&mut w, "task:t3"),
        "Criterion a3 has no readiness assessment"
    );
    w.ok("agent", assess_access("a3", true, "Known scope", None));
    save(
        &mut w,
        "task:t4",
        Some("ac:a3"),
        work("Implement access", "implementation"),
    );
    claim(&mut w, "bot", "task:t4").unwrap();
    assert_eq!(
        w.state.task_process["task:t4"].assignee.as_deref(),
        Some("bot")
    );
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
    assert_eq!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:work",
            &w.state.task_process["task:work"]
        )
        .unwrap_err()
        .to_string(),
        "Criterion ios has no readiness assessment"
    );
    let mut ready = assess("ios", true);
    ready["revision"] = json!(2);
    w.ok("agent", ready);
    let error = claim(&mut w, "agent", "task:work").unwrap_err().to_string();
    assert!(error.contains("owner review"));
    assert!(w.state.task_process["task:work"].assignee.is_none());
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
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["*"],"parent_gate":"ready"}
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
    for (n, cmd) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{n}"), &cmd).await?;
    }
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
    assert_eq!(full.1["status"]["ready"], true);
    conn.execute("UPDATE joppa.objects SET work_ready=false,process=jsonb_set(process,'{status,ready}','false') WHERE workspace=$1 AND address='ac:ios'", &[&ws]).await?;
    conn.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{projection_version}','\"8\"') WHERE workspace=$1", &[&ws]).await?;
    let stale = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    assert!(!stale.get::<_, bool>(2));
    store::backfill_board(&pool).await?;
    let repaired = conn.query_one(object, &[&ws, &"ac:ios"]).await?;
    assert!(repaired.get::<_, bool>(2));
    let repaired_process: Value = serde_json::from_str(repaired.get(3))?;
    assert_eq!(repaired_process["status"]["ready"], true);
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
    assert_eq!(serde_json::from_str::<Value>(&before)?[1], "14");
    store::backfill_board(&pool).await?;
    assert_eq!(
        conn.query_one(snapshot, &[&ws]).await?.get::<_, String>(0),
        before,
        "a current projection must not be rewritten"
    );
    Ok(())
}

// A new workspace stores an epoch at position 1 for each replay rule. A journal without the
// epoch of a rule refuses to replay: the replay cannot tell the old meaning of its events.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_journal_without_a_replay_epoch_refuses_to_replay() -> anyhow::Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let conn = pool.get().await?;
    let name: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("epochs-{}", uuid::Uuid::new_v4().simple());
    for (n, cmd) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{n}"), &cmd).await?;
    }
    let epochs: Vec<(String, i64)> = conn
        .query(
            "SELECT rule,first_position FROM joppa.replay_epochs WHERE workspace=$1 ORDER BY rule",
            &[&ws],
        )
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    let mut expected: Vec<(String, i64)> = joppa::commands::REPLAY_RULES
        .iter()
        .map(|rule| (rule.to_string(), 1))
        .collect();
    expected.sort();
    assert_eq!(epochs, expected);
    store::check_journal(&pool).await?;
    conn.execute(
        "DELETE FROM joppa.replay_epochs WHERE workspace=$1 AND rule=$2",
        &[&ws, &joppa::commands::LIST_SAVE_RESET],
    )
    .await?;
    let error = format!("{:#}", store::check_journal(&pool).await.unwrap_err());
    // Restore the epoch first, so that later tests in this database can replay every journal.
    conn.execute(
        "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) VALUES($1,$2,1)",
        &[&ws, &joppa::commands::LIST_SAVE_RESET],
    )
    .await?;
    assert!(
        error.contains(&format!(
            "workspace {ws} has no replay epoch for rule list_save_reset"
        )),
        "{error}"
    );
    store::check_journal(&pool).await?;
    Ok(())
}
