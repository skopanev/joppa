//! One example save_object command for each type of a workspace schema.
//! joppa_help shows them, so a writer sees every field before the first error.
use crate::objects::{RequiredFieldRule, TreeType};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The data fields of every example: the title, the text and the owner. Reference and
/// required fields of the type follow.
const DATA_FIELDS: [&str; 3] = ["title", "body", "owner"];

fn placeholder(field: &str, limit: usize) -> Value {
    match field {
        "title" => json!("<one concise statement (max 128 chars)>"),
        "owner" => json!("<member id>"),
        _ => json!(format!("<concise text (max {limit} chars)>")),
    }
}

/// One example for each type. Angle brackets mark the values to replace.
pub fn save_examples(types: &BTreeMap<String, TreeType>) -> Value {
    let mut examples = Map::new();
    for (kind, t) in types {
        let mut data = Map::new();
        for field in DATA_FIELDS {
            let limit = crate::field_limit::limit(t.field_max_length);
            data.insert(field.into(), placeholder(field, limit));
        }
        for (field, rule) in &t.required_fields {
            let value = match rule {
                RequiredFieldRule::Text(text) => {
                    json!(format!(
                        "<text that contains: {}>",
                        text.contains.join(", ")
                    ))
                }
                RequiredFieldRule::Presence(_) => json!("<value>"),
            };
            data.insert(field.clone(), value);
        }
        for (field, rule) in &t.references {
            let target = format!("<{}:id>", rule.target);
            data.insert(
                field.clone(),
                if rule.multiple {
                    json!([target])
                } else {
                    json!(target)
                },
            );
        }
        // An inbox object has no parent: it links to the objects that it informs.
        let parent = if t.root || t.inbox == Some(true) {
            Value::Null
        } else {
            t.parents.first().map_or(Value::Null, |p| match p.as_str() {
                "*" => json!("<parent address>"),
                p => json!(format!("<{p}:id>")),
            })
        };
        // A new object omits id: the server makes one.
        let mut example = json!({"op":"save_object","kind":kind,"revision":0,
            "parent":parent,"data":data,"links":[{"target":"<kind:id>","relation":"related"}],"reason":"<why this change>"});
        if let Some(question) = &t.question {
            example["question_checked"] = json!(question);
        }
        examples.insert(kind.clone(), example);
    }
    Value::Object(examples)
}
