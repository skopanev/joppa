//! The test rules of the tree schema: the check of a block and of one result.
use joppa::result_shape::{ResultShape, validate};
use joppa::test_rules::{legacy, validate_address};
use serde_json::{Map, Value, json};

/// Check one result with no objects for ref fields.
fn check(
    rules: &ResultShape,
    status: &str,
    extra: Map<String, Value>,
) -> anyhow::Result<joppa::result_check::Checked> {
    joppa::result_check::check(rules, status, extra, &Default::default())
}

fn extra(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

/// The older rules with other status names and a required app field.
fn app_rules() -> ResultShape {
    let mut rules = legacy();
    rules.statuses = serde_json::from_value(
        json!({"passed": "pass", "broken": "fail", "flaky": "fail", "ignored": "skip"}),
    )
    .unwrap();
    rules.fields.push(
        serde_json::from_value(json!({"name": "app", "type": "enum", "values": ["ios", "android"], "required": true, "board": true}))
            .unwrap(),
    );
    rules.fields.push(
        serde_json::from_value(
            json!({"name": "run_url", "title": "Run", "type": "text", "max_length": 10}),
        )
        .unwrap(),
    );
    rules
}

#[test]
fn the_older_rules_are_a_valid_block_with_the_four_outcomes() {
    let rules = legacy();
    validate(&rules).unwrap();
    let checked = check(&rules, "skip", Map::new()).unwrap();
    assert_eq!(checked.status, "skipped");
    assert_eq!(checked.status_name.as_deref(), Some("skip"));
    let error = check(&rules, "passed", Map::new()).unwrap_err().to_string();
    assert!(
        error.contains("status must be one of: fail, pass, skip, todo"),
        "{error}"
    );
    let checked = check(&rules, "fail", extra(json!({"message": "x".repeat(9000)}))).unwrap();
    assert_eq!(
        checked.fields.unwrap()["message"].as_str().unwrap().len(),
        8 * 1024
    );
    validate_address(&rules, "tests/login.rs > email").unwrap();
    let error = validate_address(&rules, "login > email")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("a test address needs file > name at least"),
        "{error}"
    );
}

#[test]
fn a_block_maps_its_status_names_and_keeps_its_fields() {
    let rules = app_rules();
    validate(&rules).unwrap();
    let checked = check(
        &rules,
        "flaky",
        extra(json!({"app": "ios", "run_url": "https://ci.example/123"})),
    )
    .unwrap();
    assert_eq!(checked.status, "fail");
    assert_eq!(checked.status_name.as_deref(), Some("flaky"));
    assert_eq!(
        checked.fields,
        Some(json!({"app": "ios", "run_url": "https://ci"}))
    );
    for (status, fields, error) in [
        (
            "pass",
            json!({"app": "ios"}),
            "status must be one of: broken, flaky, ignored, passed",
        ),
        ("passed", json!({}), "result field app is required"),
        (
            "passed",
            json!({"app": "web"}),
            "result field app needs one of: ios, android",
        ),
        (
            "passed",
            json!({"app": "ios", "duration_ms": "two"}),
            "result field duration_ms needs a number",
        ),
        (
            "passed",
            json!({"app": "ios", "run_url": ""}),
            "result field run_url needs a text",
        ),
        (
            "passed",
            json!({"app": "ios", "os": "linux"}),
            "unknown result field `os`",
        ),
    ] {
        let message = check(&rules, status, extra(fields.clone()))
            .unwrap_err()
            .to_string();
        assert!(message.contains(error), "{status} {fields}: {message}");
    }
}

#[test]
fn a_block_with_a_bad_rule_or_field_is_refused() {
    let base = serde_json::to_value(legacy()).unwrap();
    for (key, value, error) in [
        ("statuses", json!({}), "1 to 20 names"),
        (
            "statuses",
            json!({"ok": "pass"}),
            "needs a name that counts as fail",
        ),
        (
            "statuses",
            json!({"ok": "pass", "bad": "red"}),
            "must count as pass, fail, skip or todo",
        ),
        (
            "name",
            json!({"separator": "", "min_parts": 2, "file_extension": true, "max_length": 400}),
            "test_results.name needs a separator",
        ),
        (
            "max_tests",
            json!(0),
            "max_tests and max_results are from 1 to 1000",
        ),
        (
            "same_commit",
            json!("newest"),
            "same_commit is fail_wins or latest_wins",
        ),
        (
            "green_if_all",
            json!([]),
            "green_if_all and red_if_any list different outcomes",
        ),
        (
            "red_if_any",
            json!(["pass"]),
            "cannot be in green_if_all and red_if_any",
        ),
        (
            "failure_field",
            json!("duration_ms"),
            "failure_field names a text field",
        ),
        (
            "fields",
            json!([{"name": "status", "type": "text", "max_length": 5}]),
            "result field status is part of the record",
        ),
        (
            "fields",
            json!([{"name": "Browser", "type": "text", "max_length": 5}]),
            "uses a-z, 0-9 and _",
        ),
        (
            "fields",
            json!([{"name": "a", "type": "number"}, {"name": "a", "type": "number"}]),
            "appears twice",
        ),
        (
            "fields",
            json!([{"name": "a", "type": "date"}]),
            "type text, number, enum or ref",
        ),
        (
            "fields",
            json!([{"name": "a", "type": "enum"}]),
            "enum field a needs 1 to 50 different values",
        ),
        (
            "fields",
            json!([{"name": "a", "type": "text"}]),
            "text field a needs max_length",
        ),
        (
            "fields",
            json!([{"name": "a", "type": "text", "max_length": 5, "board": true}]),
            "board field a must be an enum or ref field",
        ),
    ] {
        let mut block = base.clone();
        block[key] = value.clone();
        if key == "fields" {
            block["failure_field"] = Value::Null;
        }
        let shape: ResultShape = serde_json::from_value(block).unwrap();
        let message = validate(&shape).unwrap_err().to_string();
        assert!(message.contains(error), "{key} {value}: {message}");
    }
    assert!(serde_json::from_value::<ResultShape>(json!({"columns": []})).is_err());
}
