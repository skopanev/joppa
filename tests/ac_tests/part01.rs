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
    assert_eq!(s.commits_behind, Some(1));
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
fn grey_without_tests_without_runs_on_skips_and_on_a_draft() {
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
    let s = criterion_status(
        &tests(&["x"]),
        true,
        Boundary::None,
        &reports,
        &[result(1, "x", "pass")],
        &joppa::test_rules::legacy(),
    );
    assert_eq!(
        (s.status.as_str(), s.reason.as_deref()),
        ("grey", Some("tests_draft"))
    );
}

#[test]
fn set_ac_tests_keeps_its_own_history_and_refuses_bad_lists() {
    let mut w = World::new();
    let ac = joppa::objects::criterion_address("ios");
    let req_revision = w.state.requirements["r"].revisions.len();
    w.ok(
        "agent",
        json!({"op":"set_ac_tests","target":ac,"tests":[OPENS],"reason":"Prove iOS access"}),
    );
    w.ok("agent", json!({"op":"set_ac_tests","target":ac,"tests":[OPENS, "a.test.ts > b"],"reason":"Add a case"}));
    let saved = &w.state.ac_tests[&ac];
    assert_eq!(saved.revisions.len(), 2);
    assert_eq!(saved.current().unwrap().actor, "agent");
    assert_eq!(
        w.state.requirements["r"].revisions.len(),
        req_revision,
        "a list save never makes a requirement revision"
    );
    for (tests, error) in [
        (
            json!(["login ok"]),
            "a test address needs file > name at least",
        ),
        (
            json!(["login > ok"]),
            "a test address needs file > name at least",
        ),
        (json!([OPENS, OPENS]), "is listed twice"),
    ] {
        let e = w
            .apply(
                "agent",
                json!({"op":"set_ac_tests","target":ac,"tests":tests,"reason":"Bad"}),
            )
            .unwrap_err();
        assert!(e.to_string().contains(error), "{e}");
    }
    // checks-listed-first-ac2: the owner saves "login > ok", which has no file.
    let e = w
        .apply(
            "owner",
            json!({"op":"set_ac_tests","target":ac,"tests":["login > ok"],"reason":"No file"}),
        )
        .unwrap_err();
    assert!(
        e.to_string()
            .contains("a test address needs file > name at least"),
        "{e}"
    );
    let e = w
        .apply(
            "outsider",
            json!({"op":"set_ac_tests","target":ac,"tests":[OPENS],"reason":"No"}),
        )
        .unwrap_err();
    assert!(e.to_string().contains("not a workspace member"), "{e}");
}
