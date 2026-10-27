//! The result shape of the tree schema: its checks and the stored values of one result.
use joppa::result_shape::{ResultShape, check, effective, validate};
use serde_json::{Map, Value, json};

fn shape(value: Value) -> ResultShape {
    serde_json::from_value(value).unwrap()
}

fn extra(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

fn browser_shape() -> ResultShape {
    shape(json!({
        "statuses": {"passed": "pass", "broken": "fail", "flaky": "fail", "ignored": "skip"},
        "fields": [
            {"name": "browser", "type": "enum", "values": ["chromium", "webkit"], "required": true},
            {"name": "run_url", "title": "Run", "type": "text"},
            {"name": "retries", "type": "number"}
        ]
    }))
}

#[test]
fn without_a_shape_the_built_in_status_names_apply() {
    let checked = check(None, "skip", Map::new()).unwrap();
    assert_eq!(checked.status, "skipped");
    assert_eq!(checked.status_name, None);
    assert_eq!(checked.fields, None);
    let error = check(None, "passed", Map::new()).unwrap_err().to_string();
    assert!(error.contains("pass, fail, skip or todo"), "{error}");
    let error = check(None, "pass", extra(json!({"browser": "webkit"})))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown result field `browser`"), "{error}");
    assert_eq!(
        effective(None),
        json!({"statuses": {"fail": "fail", "pass": "pass", "skip": "skip", "todo": "todo"}, "fields": []})
    );
}

#[test]
fn a_shape_maps_its_status_names_and_keeps_its_fields() {
    let s = browser_shape();
    validate(&s).unwrap();
    let checked = check(
        Some(&s),
        "flaky",
        extra(json!({"browser": "webkit", "retries": 2})),
    )
    .unwrap();
    assert_eq!(checked.status, "fail");
    assert_eq!(checked.status_name.as_deref(), Some("flaky"));
    assert_eq!(
        checked.fields,
        Some(json!({"browser": "webkit", "retries": 2}))
    );
    let checked = check(Some(&s), "ignored", extra(json!({"browser": "chromium"}))).unwrap();
    assert_eq!(checked.status, "skipped");
}

#[test]
fn a_result_that_does_not_fit_the_shape_is_refused() {
    let s = browser_shape();
    for (status, fields, error) in [
        (
            "pass",
            json!({"browser": "webkit"}),
            "status must be one of: broken, flaky, ignored, passed",
        ),
        ("passed", json!({}), "result field browser is required"),
        (
            "passed",
            json!({"browser": "firefox"}),
            "result field browser needs one of: chromium, webkit",
        ),
        (
            "passed",
            json!({"browser": "webkit", "retries": "two"}),
            "result field retries needs a number",
        ),
        (
            "passed",
            json!({"browser": "webkit", "run_url": ""}),
            "result field run_url needs a text",
        ),
        (
            "passed",
            json!({"browser": "webkit", "os": "linux"}),
            "unknown result field `os`",
        ),
    ] {
        let message = check(Some(&s), status, extra(fields.clone()))
            .unwrap_err()
            .to_string();
        assert!(message.contains(error), "{status} {fields}: {message}");
    }
}

#[test]
fn a_shape_with_a_bad_status_map_or_field_is_refused() {
    for (value, error) in [
        (json!({"statuses": {}}), "1 to 20 names"),
        (
            json!({"statuses": {"ok": "pass"}}),
            "needs a name that counts as fail",
        ),
        (
            json!({"statuses": {"ok": "pass", "bad": "red"}}),
            "must count as pass, fail, skip or todo",
        ),
        (
            json!({"statuses": {"o k": "pass", "bad": "fail"}}),
            "without spaces",
        ),
        (
            json!({"fields": [{"name": "status", "type": "text"}]}),
            "result field status is built-in",
        ),
        (
            json!({"fields": [{"name": "Browser", "type": "text"}]}),
            "uses a-z, 0-9 and _",
        ),
        (
            json!({"fields": [{"name": "a", "type": "text"}, {"name": "a", "type": "number"}]}),
            "appears twice",
        ),
        (
            json!({"fields": [{"name": "a", "type": "date"}]}),
            "type text, number or enum",
        ),
        (
            json!({"fields": [{"name": "a", "type": "enum"}]}),
            "enum field a needs 1 to 50 different values",
        ),
        (
            json!({"fields": [{"name": "a", "type": "enum", "values": ["x", "x"]}]}),
            "different values",
        ),
        (
            json!({"fields": [{"name": "a", "type": "text", "values": ["x"]}]}),
            "only an enum field has values",
        ),
    ] {
        let message = validate(&shape(value.clone())).unwrap_err().to_string();
        assert!(message.contains(error), "{value}: {message}");
    }
    let unknown = serde_json::from_value::<ResultShape>(json!({"columns": []}));
    assert!(unknown.is_err());
}
