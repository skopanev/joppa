//! Status from record_results reports: a skip or todo counts, and an omitted test
//! gets no result.
use joppa::ac_tests::{Boundary, Report, TestResult, criterion_status};

fn record(id: i64, commit: &str, order: i64, received_us: i64) -> Report {
    Report {
        id,
        commit: commit.into(),
        order,
        full: false,
        recorded: true,
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

fn tests(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).into()).collect()
}

#[test]
fn a_recorded_skip_or_todo_is_never_a_pass() {
    // missing-is-not-pass-ac1 and ac3: the latest record for X is skip, then todo.
    let x = tests(&["x"]);
    let reports = [record(1, "c1", 1, 10), record(2, "c1", 2, 20)];
    let rows = [result(1, "x", "pass"), result(2, "x", "skipped")];
    let s = criterion_status(&x, false, Boundary::None, &reports, &rows);
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("grey", "skipped")
    );
    let rows = [result(1, "x", "pass"), result(2, "x", "todo")];
    let s = criterion_status(&x, false, Boundary::None, &reports, &rows);
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("grey", "todo")
    );
}

#[test]
fn an_omitted_test_gets_no_result_and_is_never_missing() {
    // no-manual-pass-ac1: the record names P and F; O is omitted.
    let listed = tests(&["p", "f", "o"]);
    let reports = [record(1, "c1", 1, 10)];
    let rows = [result(1, "p", "pass"), result(1, "f", "fail")];
    let s = criterion_status(&listed, false, Boundary::None, &reports, &rows);
    let statuses: Vec<_> = s.tests.iter().map(|t| t.status.as_str()).collect();
    assert_eq!(statuses, ["pass", "fail", "not_run"]);
    assert_eq!(s.status, "red");
}

#[test]
fn records_after_a_change_combine_and_the_latest_record_decides() {
    // The E2 criterion: two records after the change give green; a later fail gives red.
    let both = tests(&["x", "y"]);
    let at = Boundary::After(100);
    let reports = [record(1, "c1", 1, 101), record(2, "c1", 2, 102)];
    let rows = [result(1, "x", "pass"), result(2, "y", "pass")];
    assert_eq!(
        criterion_status(&both, false, at, &reports, &rows).status,
        "green"
    );
    let old = [record(1, "c1", 1, 99), record(2, "c1", 2, 100)];
    assert_eq!(
        criterion_status(&both, false, at, &old, &rows).status,
        "grey"
    );
    let later = [
        record(1, "c1", 1, 101),
        record(2, "c1", 2, 102),
        record(3, "c2", 3, 103),
    ];
    let rows = [
        result(1, "x", "pass"),
        result(2, "y", "pass"),
        result(3, "x", "fail"),
    ];
    assert_eq!(
        criterion_status(&both, false, at, &later, &rows).status,
        "red"
    );
}
