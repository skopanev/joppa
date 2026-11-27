//! The History filters: only a set filter adds its indexable condition (store_history_filters.rs).
use super::*;

fn bound() -> Bound {
    Bound {
        alias: None,
        op: None,
        kind: None,
        added: None,
        excluded: None,
        target: None,
        under: None,
    }
}

// ac:SPEEDHISTORY
#[test]
fn an_unset_filter_tests_only_its_null_parameter() {
    let sql = bound().clause(6);
    assert!(
        !sql.contains("event_raw_address") && !sql.contains("e.alias"),
        "{sql}"
    );
    for n in 6..=12 {
        assert!(sql.contains(&format!("${n}::text")), "{n} {sql}");
    }
}

// ac:SPEEDHISTORY
#[test]
fn each_set_filter_reads_the_expression_of_its_index() {
    let b = Bound {
        alias: Some("p1".into()),
        op: Some("note".into()),
        kind: Some("ac".into()),
        target: Some(vec!["ac:a".into(), "ac:old".into()]),
        ..bound()
    };
    let sql = b.clause(4);
    assert!(sql.contains("(e.alias=$4)"), "{sql}");
    assert!(sql.contains("(e.command->>'op'=$5)"), "{sql}");
    assert!(
        sql.contains("(split_part(joppa.event_raw_address(e.command),':',1)=$6)"),
        "{sql}"
    );
    assert!(
        sql.contains("(joppa.event_raw_address(e.command)=ANY($9::text[]))"),
        "{sql}"
    );
    assert!(sql.contains("($10::text[] IS NULL)"), "{sql}");
    // An alias that changes the type adds its exceptions to the kind filter.
    let aliased = Bound {
        kind: Some("ac".into()),
        added: Some(vec!["task:x".into()]),
        ..bound()
    };
    let sql = aliased.clause(1);
    assert!(sql.contains("=ANY(COALESCE($4::text[],'{}'))"), "{sql}");
    assert!(!sql.contains("$4::text[] IS NULL"), "{sql}");
}
