mod common;

use common::World;
use joppa::ac_tests::{Report, TestResult, criterion_status, parse_junit};
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

fn report(id: i64, commit: &str, order: i64, full: bool, received_ms: i64) -> Report {
    Report {
        id,
        commit: commit.into(),
        order,
        full,
        received_ms,
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
    let s = criterion_status(&tests(&["x", "y"]), false, &reports, &rows);
    assert_eq!(s.status, "green");
    assert_eq!(s.commit.as_deref(), Some("c1"));
    assert_eq!(s.commits_behind, Some(1));
    // The newest commit decides, not the newest upload.
    let late_old = [report(1, "c2", 2, false, 10), report(2, "c1", 1, false, 20)];
    let rows = [result(1, "x", "fail"), result(2, "x", "pass")];
    assert_eq!(
        criterion_status(&tests(&["x"]), false, &late_old, &rows).status,
        "red"
    );
}

#[test]
fn full_run_absence_is_missing_and_red_wins_on_one_commit() {
    let reports = [report(1, "c1", 1, false, 10), report(2, "c2", 2, true, 20)];
    let s = criterion_status(&tests(&["y"]), false, &reports, &[result(1, "y", "pass")]);
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("red", "missing")
    );
    let same = [report(1, "c", 1, true, 10), report(2, "c", 1, false, 20)];
    let s = criterion_status(&tests(&["x"]), false, &same, &[result(2, "x", "pass")]);
    assert_eq!(s.tests[0].status, "missing");
    let cases = [report(1, "c", 1, false, 10)];
    let s = criterion_status(
        &tests(&["x"]),
        false,
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
        criterion_status(&[], false, &reports, &[])
            .reason
            .as_deref(),
        Some("no tests")
    );
    let s = criterion_status(&tests(&["x"]), false, &reports, &[]);
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("grey", "not_run")
    );
    let full = [report(1, "c", 1, true, 10)];
    for skipped in ["skipped", "todo"] {
        let s = criterion_status(&tests(&["x"]), false, &full, &[result(1, "x", skipped)]);
        assert_eq!(s.status, "grey");
    }
    // `bun test -t other` marks x skipped in a partial run: x keeps its earlier pass.
    let filtered = [report(1, "c1", 1, false, 10), report(2, "c2", 2, false, 20)];
    let s = criterion_status(
        &tests(&["x"]),
        false,
        &filtered,
        &[result(1, "x", "pass"), result(2, "x", "skipped")],
    );
    assert_eq!(
        (s.status.as_str(), s.commit.as_deref()),
        ("green", Some("c1"))
    );
    let s = criterion_status(&tests(&["x"]), true, &reports, &[result(1, "x", "pass")]);
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
