//! Part of `store_state`: the schema rules that the stored projections read. A schema write
//! that changes only other rules, such as a type title, a color, a badge, a text limit, a done
//! rule or the priorities, projects no object again (SKK 3666). Each rule that this list does
//! not name counts, so a new rule counts until it is named here.
use crate::model::Workspace;
use anyhow::Result;
use serde_json::Value;

/// The keys of a schema revision that no stored projection reads: the revision header and the
/// priorities, which only a read and a save check use.
const REVISION_KEYS: [&str; 6] = [
    "revision",
    "actor",
    "at",
    "position",
    "reason",
    "priorities",
];

/// The keys of a type that no stored projection reads: the board look, the text limits of a
/// save and the done rule of the dependency facts, which a read computes.
const TYPE_KEYS: [&str; 8] = [
    "title",
    "question",
    "color",
    "title_size",
    "badge",
    "field_max_length",
    "title_preview_length",
    "done",
];

/// True when both workspaces have the same projection rules in their schema.
pub(crate) fn projection_schema_same(state: &Workspace, previous: &Workspace) -> Result<bool> {
    Ok(projected(state)? == projected(previous)?)
}

/// The schema without the keys that no stored projection reads.
fn projected(w: &Workspace) -> Result<Value> {
    let mut schema = serde_json::to_value(crate::objects::schema(w))?;
    if let Some(map) = schema.as_object_mut() {
        map.retain(|key, _| !REVISION_KEYS.contains(&key.as_str()));
    }
    for t in schema["types"]
        .as_object_mut()
        .into_iter()
        .flat_map(|m| m.values_mut())
    {
        if let Some(map) = t.as_object_mut() {
            map.retain(|key, _| !TYPE_KEYS.contains(&key.as_str()));
        }
    }
    Ok(schema)
}

#[cfg(test)]
#[path = "store_state_schema_tests.rs"]
mod tests;
