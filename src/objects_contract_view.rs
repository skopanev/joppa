//! Part of `objects_typed_save`: the contract of a document, the part of its data whose change
//! resets the criteria at and below the node (rule contract_only_reset). The owner, the assignee
//! and each typed field that does not reset leave it. A reference to a source becomes the title,
//! the body and the live file hashes of that source, so a new source of the same text and the
//! same files changes no contract.
use super::*;
use std::borrow::Cow;

/// The data of a document of `kind` without the parts that reset nothing.
pub(crate) fn contract<'a>(
    w: &Workspace,
    types: &BTreeMap<String, TreeType>,
    kind: &str,
    data: &'a Value,
) -> Cow<'a, Value> {
    let Some(map) = data.as_object() else {
        return Cow::Borrowed(data);
    };
    let t = types.get(kind);
    let role = t.map_or("", |t| t.role.as_str());
    let quiet = |field: &str| {
        matches!(field, "owner" | "assignee")
            || t.and_then(|t| t.fields.get(field))
                .is_some_and(|rule| !rule.resets(role))
    };
    let mut out = serde_json::Map::new();
    for (field, value) in map {
        if quiet(field) {
            continue;
        }
        let source = t
            .and_then(|t| t.references.get(field))
            .filter(|r| types.get(&r.target).is_some_and(|target| target.source));
        let value = match source {
            Some(rule) => sources(w, &rule.target, value),
            None => value.clone(),
        };
        out.insert(field.clone(), value);
    }
    Cow::Owned(Value::Object(out))
}

/// Each source of a reference value as its title, body and live file hashes, in order.
fn sources(w: &Workspace, target: &str, value: &Value) -> Value {
    let one = |text: &str| -> Value {
        let address = match text.contains(':') {
            true => text.to_owned(),
            false => format!("{target}:{text}"),
        };
        let address = resolve_address(w, &address);
        let Some(object) = w.objects.get(address) else {
            return Value::String(text.to_owned());
        };
        let data = &object.current().data;
        let mut files: Vec<&str> = w
            .notes
            .get(address)
            .into_iter()
            .flat_map(|n| n.attachments.values())
            .filter(|a| !a.removed)
            .map(|a| a.hash.as_str())
            .collect();
        files.sort_unstable();
        serde_json::json!({"title": data["title"], "body": data["body"], "files": files})
    };
    match value {
        Value::String(text) => one(text),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| i.as_str().map_or_else(|| i.clone(), one))
                .collect(),
        ),
        other => other.clone(),
    }
}
