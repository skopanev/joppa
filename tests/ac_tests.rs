mod common;

use common::World;
use joppa::ac_tests::{Boundary, Report, TestResult, criterion_status, parse_junit};
use serde_json::json;

// Real output of `bun test` 1.4 with `[test.reporter] junit` in bunfig.toml.
const BUN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="bun test" tests="4">
  <testsuite name="sign-in.test.ts" file="sign-in.test.ts" tests="4">
    <testsuite name="passkey login" file="sign-in.test.ts" line="2" tests="4">
      <testcase name="opens the cabinet" classname="passkey login" time="0.000012" file="sign-in.test.ts" line="3" assertions="1" />
      <testcase name="refuses without passkey" classname="passkey login" time="0.000222" file="sign-in.test.ts" line="4" assertions="1">
        <failure type="AssertionError" message="Expected: 403&#10;Received: 200&#10;">AssertionError</failure>
      </testcase>
      <testcase name="skipped one" classname="passkey login" time="0" file="sign-in.test.ts" line="5" assertions="0">
        <skipped />
      </testcase>
      <testcase name="todo one" classname="passkey login" time="0" file="sign-in.test.ts" line="6" assertions="0">
        <skipped message="TODO" />
      </testcase>
    </testsuite>
  </testsuite>
</testsuites>"#;

const OPENS: &str = "sign-in.test.ts > passkey login > opens the cabinet";

#[test]
fn bun_junit_becomes_file_describe_name_addresses() {
    let results = parse_junit(BUN).unwrap();
    let view: Vec<(&str, &str)> = results
        .iter()
        .map(|r| (r.address.as_str(), r.status.as_str()))
        .collect();
    assert_eq!(
        view,
        vec![
            (OPENS, "pass"),
            (
                "sign-in.test.ts > passkey login > refuses without passkey",
                "fail"
            ),
            ("sign-in.test.ts > passkey login > skipped one", "skipped"),
            ("sign-in.test.ts > passkey login > todo one", "todo"),
        ]
    );
    assert_eq!(
        results[1].message.as_deref(),
        Some("Expected: 403\nReceived: 200\n")
    );
    assert!(parse_junit("<testsuite><testcase").is_err());
}

fn report(id: i64, commit: &str, order: i64, full: bool, received_us: i64) -> Report {
    Report {
        id,
        commit: commit.into(),
        order,
        full,
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
        },
    )
}
fn tests(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn each_test_counts_with_its_latest_result() {
    // X passes on C2 in a partial run, Y passed earlier on C1: green, shown on the older commit.
    let reports = [report(1, "c1", 1, false, 10), report(2, "c2", 2, false, 20)];
    let rows = [
        result(1, "x", "pass"),
        result(1, "y", "pass"),
        result(2, "x", "pass"),
    ];
    let s = criterion_status(&tests(&["x", "y"]), false, Boundary::None, &reports, &rows);
    assert_eq!(s.status, "green");
    assert_eq!(s.commit.as_deref(), Some("c1"));
    assert_eq!(s.commits_behind, Some(1));
    // The newest commit decides, not the newest upload.
    let late_old = [report(1, "c2", 2, false, 10), report(2, "c1", 1, false, 20)];
    let rows = [result(1, "x", "fail"), result(2, "x", "pass")];
    assert_eq!(
        criterion_status(&tests(&["x"]), false, Boundary::None, &late_old, &rows).status,
        "red"
    );
}

#[test]
fn full_run_absence_is_missing_and_red_wins_on_one_commit() {
    let reports = [report(1, "c1", 1, false, 10), report(2, "c2", 2, true, 20)];
    let s = criterion_status(
        &tests(&["y"]),
        false,
        Boundary::None,
        &reports,
        &[result(1, "y", "pass")],
    );
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("red", "missing")
    );
    let same = [report(1, "c", 1, true, 10), report(2, "c", 1, false, 20)];
    let s = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &same,
        &[result(2, "x", "pass")],
    );
    assert_eq!(s.tests[0].status, "missing");
    let cases = [report(1, "c", 1, false, 10)];
    let s = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &cases,
        &[result(1, "x", "pass"), result(1, "x", "fail")],
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
        criterion_status(&[], false, Boundary::None, &reports, &[])
            .reason
            .as_deref(),
        Some("no tests")
    );
    let s = criterion_status(&tests(&["x"]), false, Boundary::None, &reports, &[]);
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("grey", "not_run")
    );
    let full = [report(1, "c", 1, true, 10)];
    for skipped in ["skipped", "todo"] {
        let s = criterion_status(
            &tests(&["x"]),
            false,
            Boundary::None,
            &full,
            &[result(1, "x", skipped)],
        );
        assert_eq!(s.status, "grey");
    }
    // `bun test -t other` marks x skipped in a partial run: x keeps its earlier pass.
    let filtered = [report(1, "c1", 1, false, 10), report(2, "c2", 2, false, 20)];
    let s = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &filtered,
        &[result(1, "x", "pass"), result(2, "x", "skipped")],
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

// Real output of `bun test ./tests/auth/login.test.ts`: nested describe, test.each, todo, skip.
const BUN_EACH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="bun test" tests="6" assertions="4" failures="1" skipped="2" time="0.043805834">
  <testsuite name="tests/auth/login.test.ts" file="tests/auth/login.test.ts" tests="6" assertions="4" failures="1" skipped="2" time="0.01664375">
    <testsuite name="login" file="tests/auth/login.test.ts" line="2" tests="6" assertions="4" failures="1" skipped="2" time="0">
      <testsuite name="passkey" file="tests/auth/login.test.ts" line="3" tests="6" assertions="4" failures="1" skipped="2" time="0.011">
        <testcase name="works for 1" classname="passkey &gt; login" time="0.01146" file="tests/auth/login.test.ts" line="4" assertions="1" />
        <testcase name="works for 2" classname="passkey &gt; login" time="0.000071" file="tests/auth/login.test.ts" line="4" assertions="1" />
        <testcase name="same name" classname="passkey &gt; login" time="0.000075" file="tests/auth/login.test.ts" line="5" assertions="1" />
        <testcase name="same name" classname="passkey &gt; login" time="0.000794" file="tests/auth/login.test.ts" line="5" assertions="1">
          <failure type="AssertionError" message="expect(received).toBe(expected)&#10;&#10;Expected: 3&#10;Received: 2&#10;">AssertionError: expect(received).toBe(expected)&#10;&#10;Expected: 3&#10;Received: 2&#10;&#10;      at tests/auth/login.test.ts:5:68&#10;</failure>
        </testcase>
        <testcase name="later" classname="passkey &gt; login" time="0" file="tests/auth/login.test.ts" line="6" assertions="0">
          <skipped message="TODO" />
        </testcase>
        <testcase name="skipped" classname="passkey &gt; login" time="0" file="tests/auth/login.test.ts" line="7" assertions="0">
          <skipped />
        </testcase>
      </testsuite>
    </testsuite>
  </testsuite>
</testsuites>"#;

#[test]
fn bun_each_cases_share_or_split_addresses_and_paths_stay_relative() {
    let results = parse_junit(BUN_EACH).unwrap();
    let base = "tests/auth/login.test.ts > login > passkey > ";
    assert!(
        results.iter().all(|r| r.address.starts_with(base)),
        "{results:?}"
    );
    let same: Vec<&str> = results
        .iter()
        .filter(|r| r.address == format!("{base}same name"))
        .map(|r| r.status.as_str())
        .collect();
    assert_eq!(same, vec!["pass", "fail"]);
    let reports = [report(1, "c", 1, false, 10)];
    let rows: Vec<(i64, TestResult)> = results.into_iter().map(|r| (1, r)).collect();
    let s = criterion_status(
        &tests(&["tests/auth/login.test.ts > login > passkey > same name"]),
        false,
        Boundary::None,
        &reports,
        &rows,
    );
    assert_eq!(
        s.status, "red",
        "one failing case of a shared address fails it"
    );
    let dotted = BUN.replace(
        r#"name="sign-in.test.ts" file"#,
        r#"name="./sign-in.test.ts" file"#,
    );
    assert_eq!(parse_junit(&dotted).unwrap()[0].address, OPENS);
}

#[test]
fn clock_time_alone_never_changes_status() {
    // time-not-stale-ac3: the same reports, read seven days apart, give the same status.
    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    let rows = [result(1, "x", "pass")];
    let now = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &[report(1, "c", 1, true, 10)],
        &rows,
    );
    let later = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &[report(1, "c", 1, true, 10 + 7 * DAY_MS)],
        &rows,
    );
    assert_eq!(
        (now.status.as_str(), now.commit.as_deref()),
        ("green", Some("c"))
    );
    assert_eq!(
        (later.status.as_str(), later.commit.as_deref()),
        ("green", Some("c"))
    );
}

#[test]
fn an_account_without_membership_cannot_save_a_test_list() {
    // checks-listed-first-ac3: "eve" is signed in but not a member.
    let mut w = World::new();
    let ac = joppa::objects::criterion_address("ios");
    let e = w
        .apply(
            "eve",
            json!({"op":"set_ac_tests","target":ac,"tests":[OPENS],"reason":"No"}),
        )
        .unwrap_err();
    assert!(e.to_string().contains("not a workspace member"), "{e}");
    assert!(!w.state.ac_tests.contains_key(&ac));
}

#[test]
fn a_task_has_no_test_list_of_its_own() {
    // foreign-check-ignored-ac1
    let mut w = World::new();
    w.ok("owner", json!({"op":"task","req":"r","revision":1,"id":"t1","title":"Build iOS access","acs":["ios"],"projects":[],"purpose":"implementation","contract":"Implement and provide evidence"}));
    let e = w
        .apply("agent", json!({"op":"set_ac_tests","target":"task:r/t1","tests":[OPENS],"reason":"Wrong target"}))
        .unwrap_err();
    assert!(
        e.to_string()
            .contains("tests are listed on criteria, not on tasks"),
        "{e}"
    );
}

#[test]
fn reports_at_or_before_a_boundary_do_not_count_and_partial_runs_no_longer_combine() {
    // check-list-frozen-ac2: the criterion was reset at 100.
    let at = Boundary::After(100);
    let both = tests(&["x", "y"]);
    let pass = |id| [result(id, "x", "pass"), result(id, "y", "pass")];
    let old = [report(1, "c1", 1, true, 99), report(2, "c1", 1, true, 100)];
    let rows: Vec<_> = pass(1).into_iter().chain(pass(2)).collect();
    let s = criterion_status(&both, false, Boundary::None, &old, &rows);
    assert_eq!(s.status, "green", "without a boundary every report counts");
    let s = criterion_status(&both, false, at, &old, &rows);
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("grey", "not_run")
    );
    // Two partial runs after the boundary pass every test together: still grey.
    let partial = [
        report(3, "c2", 2, false, 101),
        report(4, "c3", 3, false, 102),
    ];
    let rows = [result(3, "x", "pass"), result(4, "y", "pass")];
    assert_eq!(
        criterion_status(&both, false, Boundary::None, &partial, &rows).status,
        "green"
    );
    let s = criterion_status(&both, false, at, &partial, &rows);
    assert_eq!(s.status, "grey");
    // One clean full run after the boundary that passes every test is green.
    let full = [report(5, "c4", 4, true, 101)];
    assert_eq!(
        criterion_status(&both, false, at, &full, &pass(5)).status,
        "green"
    );
    // An eligible failure stays red, also on a draft list.
    let failed = [result(5, "x", "fail"), result(5, "y", "pass")];
    assert_eq!(
        criterion_status(&both, false, at, &full, &failed).status,
        "red"
    );
    assert_eq!(
        criterion_status(&both, true, at, &full, &failed).status,
        "red"
    );
    // check-list-frozen-ac4: an eligible full run decides a draft list after the change.
    let s = criterion_status(&both, true, at, &full, &pass(5));
    assert_eq!((s.status.as_str(), s.tests_draft), ("green", true));
    // Without an eligible full run, or without a boundary, a draft stays a draft.
    let rows = [result(3, "x", "pass"), result(4, "y", "pass")];
    let s = criterion_status(&both, true, at, &partial, &rows);
    assert_eq!(s.reason.as_deref(), Some("tests_draft"));
    let s = criterion_status(&both, true, Boundary::None, &full, &pass(5));
    assert_eq!(s.reason.as_deref(), Some("tests_draft"));
    // A report first received before the boundary counts after an identical repeat.
    let mut repeated = report(1, "c1", 1, true, 99);
    repeated.receipt_us = 101;
    let s = criterion_status(&both, true, at, &[repeated], &pass(1));
    assert_eq!(s.status, "green");
    // A boundary that cannot be read gives grey, whatever the reports say.
    assert_eq!(Boundary::read(Some("not a time")), Boundary::Unreadable);
    let s = criterion_status(&both, false, Boundary::Unreadable, &full, &pass(5));
    assert_eq!(s.status, "grey");
    assert_eq!(Boundary::read(None), Boundary::None);
    assert_eq!(
        Boundary::read(Some("2026-10-14 18:20:01.000123+00")),
        Boundary::After(1_792_002_001_000_123)
    );
}
