//! Part of `objects`: the status of a saved input, such as a Note.
use super::*;

/// A new input is unprocessed. A change of the source of a processed input makes it
/// unprocessed again.
pub(super) fn input_status(
    w: &Workspace,
    kind: &str,
    old: Option<&Object>,
    saved_data: &mut Value,
) {
    // A new signal is unprocessed until someone decides on it.
    if input_type(w, kind) && saved_data["status"].is_null() {
        saved_data["status"] = json!("unprocessed");
    }
    if input_type(w, kind)
        && let Some(old) = old
    {
        let source = |value: &Value| {
            let mut value = value.clone();
            if let Some(fields) = value.as_object_mut() {
                fields.remove("status");
                fields.remove("owner");
                // A decision is a record about the signal, not a change of the signal.
                fields.remove("decision");
            }
            value
        };
        if saved_data["status"] == "processed" && source(&old.current().data) != source(saved_data)
        {
            saved_data["status"] = json!("unprocessed");
        }
    }
}
