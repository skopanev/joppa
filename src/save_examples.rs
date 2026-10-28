//! One example save_object command for each type of a workspace schema.
//! joppa_help shows them, so a writer sees every field before the first error.
use crate::objects::{RequiredFieldRule, TreeType};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The data fields that the server reads for a type, from its name or its role.
fn data_fields(kind: &str, t: &TreeType) -> &'static [&'static str] {
    if let Some(fields) = crate::objects::native_document_fields(kind) {
        return fields;
    }
    match t.role.as_str() {
        "requirement" => &["title", "body", "owner", "consumers"],
        "ac" => &[
            "title",
            "description",
            "owner",
            "decision_required",
            "depends_on",
            "before_launch",
        ],
        "task" => &["title", "owner", "contract", "projects", "purpose"],
        _ => &["title", "owner", "description"],
    }
}

fn placeholder(field: &str) -> Value {
    match field {
        "title" => json!("<one short statement, at most the title_preview_length>"),
        "owner" => json!("<member id>"),
        "consumers" | "depends_on" | "projects" => json!([]),
        "decision_required" | "before_launch" | "archived" => json!(false),
        _ => json!("<text, at most 1024 characters>"),
    }
}

/// One example for each type. Angle brackets mark the values to replace.
pub fn save_examples(types: &BTreeMap<String, TreeType>) -> Value {
    let mut examples = Map::new();
    for (kind, t) in types {
        let mut data = Map::new();
        for field in data_fields(kind, t) {
            // A requirement names no consumer, see consumer_migration.
            if t.role == "requirement"
                && *field == "consumers"
                && !t.references.contains_key(*field)
            {
                continue;
            }
            data.insert((*field).into(), placeholder(field));
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
