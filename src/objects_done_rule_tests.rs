//! The checks of the done rule of a type (objects_done_rule.rs).
use super::*;

fn step(done: Value) -> TreeType {
    serde_json::from_value(json!({"title": "Step", "role": "task", "done": done,
        "fields": {"stage": {"type": "enum", "values": ["todo", "done"]}}}))
    .unwrap()
}

fn refusal(done: Value) -> String {
    check_done_rule(&step(done)).unwrap_err().to_string()
}

#[test]
fn accepts_typed_and_free_values() {
    assert!(check_done_rule(&step(json!({"stage": "done"}))).is_ok());
    assert!(check_done_rule(&step(json!({"result": "shipped", "flag": true}))).is_ok());
    let none: TreeType = serde_json::from_value(json!({"title": "T", "role": "task"})).unwrap();
    assert!(check_done_rule(&none).is_ok());
    // A type without a done rule serializes as before.
    assert!(serde_json::to_value(&none).unwrap().get("done").is_none());
}

#[test]
fn refuses_bad_rules() {
    assert_eq!(refusal(json!({})), "done rule must name at least one field");
    assert_eq!(
        refusal(json!({"stage": "nope"})),
        "done rule: stage is one of todo, done"
    );
    assert_eq!(
        refusal(json!({"stage": null})),
        "done rule value of stage must not be null"
    );
    for field in ["depends_on", "relations", "links"] {
        let text = format!("done rule field {field} must be a data field");
        assert_eq!(refusal(json!({ field: "x" })), text);
    }
    assert!(refusal(json!({"bad key": 1})).starts_with("invalid id"));
}
