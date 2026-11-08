// Part 1 of tests/ac_tests.rs, included there, so the test names stay unchanged.

const OPENS: &str = "sign-in.test.ts > passkey login > opens the cabinet";

/// A stored report. `recorded` marks a joppa_record_results record: its skips count.
fn report(id: i64, commit: &str, order: i64, recorded: bool, received_us: i64) -> Report {
    Report {
        id,
        commit: commit.into(),
        order,
        recorded,
        received_us,
        receipt_us: received_us,
    }
}

fn result(id: i64, address: &str, status: &str) -> (i64, TestResult) {
    (
        id,
        TestResult {
            address: address.into(),
            status: status.into(),
            duration_ms: None,
            message: None,
            status_name: None,
            fields: None,
        },
    )
}

fn tests(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn each_test_counts_with_its_latest_result() {
    // X passes on C2 in a record, Y passed earlier on C1: green, shown on the older commit.
    let reports = [report(1, "c1", 1, false, 10), report(2, "c2", 2, false, 20)];
    let rows = [
        result(1, "x", "pass"),
        result(1, "y", "pass"),
        result(2, "x", "pass"),
    ];
    let s = criterion_status(
        &tests(&["x", "y"]),
        false,
        Boundary::None,
        &reports,
        &rows,
        &joppa::test_rules::legacy(),
    );
    assert_eq!(s.status, "green");
    assert_eq!(s.commit.as_deref(), Some("c1"));
    assert!(
        !serde_json::to_string(&s)
            .unwrap()
            .contains("commits_behind")
    );
    // The higher order decides, not the later receipt.
    let late_old = [report(1, "c2", 2, false, 10), report(2, "c1", 1, false, 20)];
    let rows = [result(1, "x", "fail"), result(2, "x", "pass")];
    assert_eq!(
        criterion_status(
            &tests(&["x"]),
            false,
            Boundary::None,
            &late_old,
            &rows,
            &joppa::test_rules::legacy()
        )
        .status,
        "red"
    );
}

#[test]
fn one_failing_case_of_a_test_makes_it_red() {
    let cases = [report(1, "c", 1, false, 10)];
    let s = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &cases,
        &[result(1, "x", "pass"), result(1, "x", "fail")],
        &joppa::test_rules::legacy(),
    );
    assert_eq!(
        s.status, "red",
        "every case of a parametrized test must pass"
    );
}

#[test]
fn grey_without_tests_without_runs_and_on_skips() {
    let reports = [report(1, "c", 1, false, 10)];
    assert_eq!(
        criterion_status(
            &[],
            false,
            Boundary::None,
            &reports,
            &[],
            &joppa::test_rules::legacy()
        )
        .reason
        .as_deref(),
        Some("no tests")
    );
    let s = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &reports,
        &[],
        &joppa::test_rules::legacy(),
    );
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("grey", "not_run")
    );
    let recorded = [report(1, "c", 1, true, 10)];
    for skipped in ["skipped", "todo"] {
        let s = criterion_status(
            &tests(&["x"]),
            false,
            Boundary::None,
            &recorded,
            &[result(1, "x", skipped)],
            &joppa::test_rules::legacy(),
        );
        assert_eq!(s.status, "grey");
    }
    // An older stored report marks x skipped: x keeps its earlier pass.
    let filtered = [report(1, "c1", 1, false, 10), report(2, "c2", 2, false, 20)];
    let s = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &filtered,
        &[result(1, "x", "pass"), result(2, "x", "skipped")],
        &joppa::test_rules::legacy(),
    );
    assert_eq!(
        (s.status.as_str(), s.commit.as_deref()),
        ("green", Some("c1"))
    );
}

/// A world after the journal event migrate_test_nodes, which the store writes.
fn migrated() -> World {
    let mut w = World::new();
    w.legacy("owner", json!({"op":"migrate_test_nodes"}));
    w
}

/// A save of one test node `id` under `ac`.
fn bind(ac: &str, id: &str, test: &str) -> serde_json::Value {
    json!({"op":"save_object","kind":"test","id":id,"revision":0,"parent":ac,
        "data":{"title":test,"owner":"owner"},"reason":"Prove iOS access"})
}

/// The names of the present test nodes under `ac`.
fn names(w: &World, ac: &str) -> Vec<String> {
    let names = joppa::test_nodes::names_by_parent(&w.state);
    names.get(ac).cloned().unwrap_or_default()
}

// ac:vtr2e65t
#[test]
fn a_test_node_save_never_revises_the_requirement_and_takes_a_business_title() {
    let mut w = migrated();
    let ac = joppa::objects::criterion_address("ios");
    let req_revision = w.state.requirements["r"].revisions.len();
    w.ok("agent", bind(&ac, "opens", OPENS));
    w.ok("agent", bind(&ac, "b", "a.test.ts > b"));
    assert_eq!(names(&w, &ac), ["a.test.ts > b", OPENS]);
    assert_eq!(w.state.objects["test:opens"].current().actor, "agent");
    assert_eq!(
        w.state.requirements["r"].revisions.len(),
        req_revision,
        "a test save never makes a requirement revision"
    );
    // No name rule checks a title: the agent and the owner save business titles.
    for (actor, id, name) in [
        ("agent", "refund", "A refund over the limit is refused"),
        ("agent", "login", "login ok"),
        ("owner", "plain", "login > ok"),
    ] {
        w.ok(actor, bind(&ac, id, name));
    }
    assert_eq!(names(&w, &ac).len(), 5);
    for id in ["refund", "login", "plain"] {
        w.ok("owner", json!({"op":"archive_object","target":format!("test:{id}"),
            "revision":1,"reason":"Keep two tests"}));
    }
    // A test list is refused: in a document and as the removed operation.
    let e = w
        .apply(
            "agent",
            json!({"op":"set_ac_tests","target":ac,"tests":[OPENS],"reason":"Old"}),
        )
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "set_ac_tests was removed; use save_object with kind test"
    );
    let e = w.apply("outsider", bind(&ac, "no", OPENS)).unwrap_err();
    assert!(e.to_string().contains("not a workspace member"), "{e}");
    assert_eq!(names(&w, &ac), ["a.test.ts > b", OPENS]);
}
