// Part 1 of tests/generic_tasks.rs, included there, so the test names stay unchanged.

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set the document contract"}));
}

fn document(title: &str) -> Value {
    json!({"title":title,"owner":"owner"})
}

fn contract(title: &str) -> Value {
    json!({"title":title,"owner":"owner","contract":"Implement the specified document behavior","purpose":"implementation","projects":[]})
}

fn configured() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Generic specifications"}),
    );
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    );
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"other","role":"agent"}),
    );
    let mut types: std::collections::BTreeMap<String, TreeType> = std::collections::BTreeMap::new();
    for (kind, role, root, parents) in [
        ("spec", "document", true, vec!["spec"]),
        ("task", "task", false, vec!["*"]),
        ("work", "task", true, vec!["*"]),
    ] {
        types.insert(
            kind.into(),
            TreeType {
                question: None,
                index: None,
                references: Default::default(),
                required_fields: Default::default(),
                parent_decision: false,
                status: None,
                status_order: None,
                process_fields: None,
                inbox: None,
                launch_blocker: None,
                color: None,
                title_size: None,
                badge: None,
                title_preview_length: None,
                field_max_length: None,
                title: kind.into(),
                role: role.into(),
                root,
                parents: parents.into_iter().map(String::from).collect(),
                parent_gate: None,
            },
        );
    }
    w.ok("owner",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Use specifications with any depth"}));
    save(&mut w, "spec:root", None, document("Root specification"));
    save(
        &mut w,
        "spec:child",
        Some("spec:root"),
        document("Nested specification"),
    );
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    w
}

fn action(w: &mut World, actor: &str, key: &str, op: &str, extra: Value) {
    let mut cmd = json!({"op":op,"id":key,"revision":w.state.objects[key].current().revision});
    for (key, value) in extra.as_object().unwrap() {
        cmd[key] = value.clone();
    }
    w.ok(actor, cmd);
}

// one-task-per-agent-ac1
#[test]
fn an_agent_holds_only_one_active_task_across_configured_types() {
    let mut w = configured();
    for key in ["task:one", "work:two"] {
        save(
            &mut w,
            key,
            Some("spec:root"),
            contract("Implement the contract"),
        );
    }
    action(&mut w, "agent", "task:one", "claim_task", json!({}));
    let second = json!({"op":"claim_task","id":"work:two","revision":w.state.objects["work:two"].current().revision});
    let position = w.position;
    let error = w.apply("agent", second.clone()).unwrap_err();
    assert_eq!(error.to_string(), "agent already holds task task:one");
    assert_eq!(w.position, position);
    assert!(w.state.task_process["work:two"].assignee.is_none());
    w.ok("other", second);
    action(
        &mut w,
        "agent",
        "task:one",
        "complete_task",
        json!({"evidence":"The contract works"}),
    );
    action(
        &mut w,
        "other",
        "work:two",
        "release_task",
        json!({"reason":"Return the work"}),
    );
    action(&mut w, "agent", "work:two", "claim_task", json!({}));
}

#[test]
fn task_uses_its_schema_parent_and_own_revision_without_req_or_ac() {
    let mut w = configured();
    save(
        &mut w,
        "task:one",
        Some("spec:child"),
        contract("Implement the child"),
    );
    save(
        &mut w,
        "work:two",
        Some("task:one"),
        contract("Implement a nested task"),
    );
    assert!(objects::within(&w.state, "work:two", "spec:root"));
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "work:two",
            &w.state.task_process["work:two"]
        )
        .is_ok()
    );
    action(&mut w, "agent", "task:one", "claim_task", json!({}));
    w.no(
        "other",
        json!({"op":"claim_task","id":"task:one","revision":1}),
    );
    assert!(
        tasks::claimable(
            &w.state,
            "other",
            "work:two",
            &w.state.task_process["work:two"]
        )
        .is_ok()
    );
    action(&mut w, "other", "work:two", "claim_task", json!({}));
    action(
        &mut w,
        "other",
        "work:two",
        "complete_task",
        json!({"evidence":"The child change is complete"}),
    );
    action(
        &mut w,
        "agent",
        "task:one",
        "complete_task",
        json!({"evidence":"The specified behavior works"}),
    );
    assert_eq!(objects::process(&w.state, "task:one")["status"], "done");
    assert_ne!(objects::signal(&w.state, "task:one")["tone"], "green");
    assert!(w.state.requirements.is_empty());
    assert_eq!(w.state.objects["task:one"].revisions.len(), 3);
    objects::validate_tree(&w.state).unwrap();
}

#[test]
fn old_journal_claims_survive_and_new_claims_obey_the_agent_limit() {
    let mut w = configured();
    for key in ["task:one", "work:two", "task:three"] {
        save(
            &mut w,
            key,
            Some("spec:root"),
            contract("Implement the contract"),
        );
    }
    for key in ["task:one", "work:two"] {
        let cmd: Command =
            serde_json::from_value(json!({"op":"claim_task","id":key,"revision":1})).unwrap();
        commands::apply_replay(
            &mut w.state,
            "agent",
            &cmd,
            w.position + 1,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        w.position += 1;
    }
    assert_eq!(
        w.state.task_process["task:one"].assignee.as_deref(),
        Some("agent")
    );
    assert_eq!(
        w.state.task_process["work:two"].assignee.as_deref(),
        Some("agent")
    );
    let error = w
        .apply(
            "agent",
            json!({"op":"claim_task","id":"task:three","revision":1}),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "agent already holds task task:one");
}
