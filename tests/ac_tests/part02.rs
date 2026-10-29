// Part 2 of tests/ac_tests.rs, included there, so the test names stay unchanged.

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
        &joppa::test_rules::legacy(),
    );
    let later = criterion_status(
        &tests(&["x"]),
        false,
        Boundary::None,
        &[report(1, "c", 1, true, 10 + 7 * DAY_MS)],
        &rows,
        &joppa::test_rules::legacy(),
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
    w.legacy("owner", json!({"op":"task","req":"r","revision":1,"id":"t1","title":"Build iOS access","acs":["ios"],"projects":[],"purpose":"implementation","contract":"Implement and provide evidence"}));
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
    let s = criterion_status(
        &both,
        false,
        Boundary::None,
        &old,
        &rows,
        &joppa::test_rules::legacy(),
    );
    assert_eq!(s.status, "green", "without a boundary every report counts");
    let s = criterion_status(&both, false, at, &old, &rows, &joppa::test_rules::legacy());
    assert_eq!(
        (s.status.as_str(), s.tests[0].status.as_str()),
        ("grey", "not_run")
    );
    // Two records after the boundary pass every test together: green, because
    // the latest eligible result of each test decides.
    let partial = [
        report(3, "c2", 2, false, 101),
        report(4, "c3", 3, false, 102),
    ];
    let rows = [result(3, "x", "pass"), result(4, "y", "pass")];
    assert_eq!(
        criterion_status(
            &both,
            false,
            Boundary::None,
            &partial,
            &rows,
            &joppa::test_rules::legacy()
        )
        .status,
        "green"
    );
    let s = criterion_status(
        &both,
        false,
        at,
        &partial,
        &rows,
        &joppa::test_rules::legacy(),
    );
    assert_eq!(s.status, "green");
    // One record after the boundary that passes every test is green.
    let full = [report(5, "c4", 4, true, 101)];
    assert_eq!(
        criterion_status(
            &both,
            false,
            at,
            &full,
            &pass(5),
            &joppa::test_rules::legacy()
        )
        .status,
        "green"
    );
    // An eligible failure stays red, also on a draft list.
    let failed = [result(5, "x", "fail"), result(5, "y", "pass")];
    assert_eq!(
        criterion_status(
            &both,
            false,
            at,
            &full,
            &failed,
            &joppa::test_rules::legacy()
        )
        .status,
        "red"
    );
    assert_eq!(
        criterion_status(
            &both,
            true,
            at,
            &full,
            &failed,
            &joppa::test_rules::legacy()
        )
        .status,
        "red"
    );
    // check-list-frozen-ac4: an eligible record decides a draft list after the change.
    let s = criterion_status(
        &both,
        true,
        at,
        &full,
        &pass(5),
        &joppa::test_rules::legacy(),
    );
    assert_eq!((s.status.as_str(), s.tests_draft), ("green", true));
    // After the change, records that give every listed test a result decide a draft.
    let rows = [result(3, "x", "pass"), result(4, "y", "pass")];
    let s = criterion_status(
        &both,
        true,
        at,
        &partial,
        &rows,
        &joppa::test_rules::legacy(),
    );
    assert_eq!((s.status.as_str(), s.tests_draft), ("green", true));
    // A draft with a listed test that has no eligible result, or without a boundary, stays a draft.
    let s = criterion_status(
        &both,
        true,
        at,
        &partial[..1],
        &rows[..1],
        &joppa::test_rules::legacy(),
    );
    assert_eq!(s.reason.as_deref(), Some("tests_draft"));
    let s = criterion_status(
        &both,
        true,
        Boundary::None,
        &full,
        &pass(5),
        &joppa::test_rules::legacy(),
    );
    assert_eq!(s.reason.as_deref(), Some("tests_draft"));
    // A report first received before the boundary counts after an identical repeat.
    let mut repeated = report(1, "c1", 1, true, 99);
    repeated.receipt_us = 101;
    let s = criterion_status(
        &both,
        true,
        at,
        &[repeated],
        &pass(1),
        &joppa::test_rules::legacy(),
    );
    assert_eq!(s.status, "green");
    // A boundary that cannot be read gives grey, whatever the reports say.
    assert_eq!(Boundary::read(Some("not a time")), Boundary::Unreadable);
    let s = criterion_status(
        &both,
        false,
        Boundary::Unreadable,
        &full,
        &pass(5),
        &joppa::test_rules::legacy(),
    );
    assert_eq!(s.status, "grey");
    assert_eq!(Boundary::read(None), Boundary::None);
    assert_eq!(
        Boundary::read(Some("2026-10-14 18:20:01.000123+00")),
        Boundary::After(1_792_002_001_000_123)
    );
}
