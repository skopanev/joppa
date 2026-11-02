use serde_json::json;

fn parse_default(
    args: &serde_json::Value,
) -> anyhow::Result<(String, Vec<crate::ac_tests::TestResult>)> {
    super::parse(args, &crate::test_rules::legacy(), &Default::default())
}

#[test]
fn a_record_maps_skip_and_keeps_the_failure_message() {
    let (commit, rows) = parse_default(&json!({"workspace":"w","commit":"abcdef1","results":[
        {"test":"a","status":"pass"},{"test":"b","status":"fail","message":"expected 2, got 3"},
        {"test":"c","status":"skip"},{"test":"d","status":"todo","duration_ms":4}]}))
    .unwrap();
    assert_eq!(commit, "abcdef1");
    let statuses: Vec<_> = rows.iter().map(|r| r.status.as_str()).collect();
    assert_eq!(statuses, ["pass", "fail", "skipped", "todo"]);
    assert_eq!(
        rows[1].fields,
        Some(json!({"message": "expected 2, got 3"}))
    );
    assert_eq!(rows[3].fields, Some(json!({"duration_ms": 4})));
}

#[test]
fn a_record_takes_1000_results_and_refuses_more() {
    let rows = |n: usize| -> Vec<_> {
        (0..n)
            .map(|i| json!({"test": format!("t{i}"), "status": "pass"}))
            .collect()
    };
    let (_, parsed) = parse_default(&json!({"commit":"abcdef1","results":rows(1000)})).unwrap();
    assert_eq!(parsed.len(), 1000);
    let error = parse_default(&json!({"commit":"abcdef1","results":rows(1001)})).unwrap_err();
    assert!(
        error.to_string().contains("at most 1000 results"),
        "{error}"
    );
}

#[test]
fn a_record_refuses_actor_at_unknown_fields_and_bad_input() {
    for (input, error) in [
        (
            json!({"commit":"abcdef1","results":[{"test":"a","status":"pass"}],"actor":"x"}),
            "actor comes from the server",
        ),
        (
            json!({"commit":"abcdef1","results":[{"test":"a","status":"pass"}],"at":"2020-01-01T00:00:00Z"}),
            "at comes from the server",
        ),
        (
            json!({"commit":"abcdef1","results":[{"test":"a","status":"pass","actor":"x"}]}),
            "invalid record_results input",
        ),
        (
            json!({"commit":"ABCDEF1","results":[{"test":"a","status":"pass"}]}),
            "lowercase git sha",
        ),
        (
            json!({"commit":"abcdef1","results":[]}),
            "at least one test",
        ),
        (
            json!({"commit":"abcdef1","results":[{"test":"a","status":"passed"}]}),
            "status must be one of: fail, pass, skip, todo",
        ),
    ] {
        let message = parse_default(&input).unwrap_err().to_string();
        assert!(message.contains(error), "{input}: {message}");
    }
}
