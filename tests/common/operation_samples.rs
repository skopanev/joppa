//! Type-correct sample values for the help-required fields of public operations.
use serde_json::{Value, json};

pub fn sample(op: &str, field: &str) -> Value {
    match field {
        "revision" | "expected_revision" | "size" => json!(0),
        "types" | "subject" | "data" => json!({}),
        "tests" | "expected" | "ids" | "subject_keys" | "projects" | "acs" | "checks" => json!([]),
        "archived" | "discovery_complete" => json!(false),
        "assessment" => json!({"ready":true,"reason":"The contract is clear"}),
        "evidence" if op == "result" => {
            json!({"check":"c1","outcome":"pass","observed":["Seen"],"note":"Checked"})
        }
        _ => json!("value"),
    }
}

/// A call of `op` built from only its help-required fields.
pub fn call(op: &str, required: &[&str]) -> Value {
    let mut command = json!({"op":op});
    for field in required {
        command[*field] = sample(op, field);
    }
    command
}
