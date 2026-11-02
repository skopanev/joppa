// Part 1 of tests/readiness_metadata.rs, included there, so the test names stay unchanged.

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok(
        "owner",
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "parent":parent,"data":data,"reason":"Set the current contract"}),
    );
}

fn work(title: &str) -> Value {
    json!({"title":title,"owner":"legal","body":"Implement the specified behavior","projects":[]})
}

fn native() -> World {
    let mut w = World::new();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    joppa::body_migration::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    save(
        &mut w,
        "task:work",
        Some("ac:ios"),
        work("Implement access"),
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
    joppa::body_migration::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    for (task, ac) in [
        ("task:t1", "ac:a1"),
        ("task:t2", "ac:a2"),
        ("task:t3", "ac:a3"),
    ] {
        save(
            &mut w,
            task,
            Some(ac),
            work("Implement access"),
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
        work("Implement access"),
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

