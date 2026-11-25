//! Part of `objects`: the data that save_object stores, moved from objects_saved.rs unchanged.
use super::*;

/// The data of the new revision: the Task data of a legacy Task save, the projected document of
/// a typed specification with the sent fields, else the sent data.
pub(crate) fn saved_data(
    w: &Workspace,
    (kind, key): (&str, &str),
    data: &Value,
    projected: Value,
    task_data: Option<Value>,
) -> Value {
    if let Some(data) = task_data {
        data
    } else if !w.document_overrides.contains(key)
        && (w.documents_migrated
            && matches!(schema(w).types[kind].role.as_str(), "requirement" | "ac")
            || !w.documents_migrated && legacy_defaults().contains_key(kind) && kind != "note")
    {
        if w.schema_migrated {
            let mut normalized = projected;
            normalized.as_object_mut().unwrap().retain(|field, _| {
                data.get(field).is_some()
                    || document_adapter_fields(w, kind)
                        .is_some_and(|fields| fields.contains(&field.as_str()))
            });
            for (field, value) in data.as_object().unwrap() {
                normalized[field] = value.clone();
            }
            if !w.documents_migrated {
                for field in schema_migration_defaults()
                    .get(kind)
                    .into_iter()
                    .flat_map(|t| t.references.keys())
                {
                    if !schema(w).types[kind].references.contains_key(field)
                        && data.get(field).is_none()
                    {
                        normalized.as_object_mut().unwrap().remove(field);
                    }
                }
            }
            normalized
        } else {
            projected
        }
    } else {
        data.clone()
    }
}
