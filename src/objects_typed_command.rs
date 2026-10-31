//! Part of `objects`: `typed_command` and the items after it, moved from objects.rs unchanged.
use super::*;

pub(crate) fn typed_command(
    w: &Workspace,
    kind: &str,
    id: &str,
    parent: Option<&str>,
    data: &Value,
    reason: &str,
) -> Result<Option<Command>> {
    let configured = schema(w);
    let role = configured
        .types
        .get(kind)
        .context("unknown object type")?
        .role
        .as_str();
    let workflow = if w.documents_migrated {
        if !matches!(role, "requirement" | "ac") {
            return Ok(None);
        }
        role
    } else if matches!(role, "requirement" | "ac") {
        role
    } else if matches!(kind, "requirement" | "ac") {
        return Ok(None);
    } else {
        kind
    };
    let mut native_data = data.clone();
    if w.documents_migrated && workflow == "requirement" {
        native_data["consumers"] = json!([]);
    }
    if w.schema_migrated
        && !w.documents_migrated
        && workflow == "requirement"
        && let Some(value) = data.get("consumers").and_then(Value::as_str)
    {
        native_data["consumers"] = json!([value]);
    }
    let data = &native_data;
    let exists = w.objects.contains_key(&address(kind, id));
    let allowed = native_document_fields(workflow).map(|fields| match w.body_migrated {
        true => crate::body_migration::body_fields(fields),
        false => fields,
    });
    if let Some(allowed) = allowed {
        for field in data.as_object().context("data must be an object")?.keys() {
            ensure!(
                allowed.contains(&field.as_str()) || w.schema_migrated && !process_field(field),
                "unknown document field {field}; use process commands for evidence"
            );
        }
    }
    workflow_command(w, workflow, kind, id, parent, data, reason, exists)
}
