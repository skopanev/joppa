//! Per-node changes of the tree schema.
//!
//! `set_tree_schema` takes the whole type map in `types`, or node changes in
//! `put_types` and `remove_types`. The node changes apply to the current schema.
//! The result goes through the same validation as a whole map.

use crate::model::Workspace;
use crate::objects::TreeType;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

/// The type map that a `set_tree_schema` command asks for.
///
/// - `types` replaces the whole map.
/// - `put_types` adds a node or replaces one node completely.
/// - `remove_types` deletes nodes.
/// - Without any of them, the current map stays and only other schema fields change.
pub fn requested_types(
    w: &Workspace,
    types: Option<&BTreeMap<String, TreeType>>,
    put_types: Option<&BTreeMap<String, TreeType>>,
    remove_types: Option<&Vec<String>>,
) -> Result<BTreeMap<String, TreeType>> {
    if let Some(types) = types {
        ensure!(
            put_types.is_none() && remove_types.is_none(),
            "send types for the whole map, or put_types and remove_types for single types"
        );
        return Ok(types.clone());
    }
    let mut merged = crate::objects::schema(w).types;
    if let Some(remove) = remove_types {
        ensure!(!remove.is_empty(), "remove_types needs at least one type");
        for kind in remove {
            ensure!(
                put_types.is_none_or(|put| !put.contains_key(kind)),
                "a type cannot be in put_types and in remove_types"
            );
            ensure!(
                merged.remove(kind).is_some(),
                "remove_types names an unknown type: {kind}"
            );
        }
    }
    if let Some(put) = put_types {
        ensure!(!put.is_empty(), "put_types needs at least one type");
        for (kind, node) in put {
            merged.insert(kind.clone(), node.clone());
        }
    }
    Ok(merged)
}

/// The board fields of a type. The MCP tree does not give them (ac:bwc9qvme).
pub const UI_FIELDS: [&str; 4] = ["color", "badge", "index", "title_size"];

/// A `set_tree_schema` command over MCP (ac:iolhwin0): each type in `types` or `put_types` that
/// omits a board field takes the value of the same type in `current`, the type map of the
/// current schema. A field sent as null stays null and clears the value. A typed field that
/// omits its board flag takes the flag of the same field of that type, and board: false clears
/// it (ac:sigd1mxp). The journal stores the filled command, so a replay gives the same revision.
pub fn keep_ui_fields(command: &mut serde_json::Value, current: &serde_json::Value) {
    for map in ["types", "put_types"] {
        let sent = command.get_mut(map).and_then(|m| m.as_object_mut());
        for (kind, node) in sent.into_iter().flatten() {
            let (Some(node), Some(stored)) = (node.as_object_mut(), current[kind].as_object())
            else {
                continue;
            };
            for field in UI_FIELDS {
                if let (false, Some(value)) = (node.contains_key(field), stored.get(field)) {
                    node.insert(field.into(), value.clone());
                }
            }
            let fields = node.get_mut("fields").and_then(|f| f.as_object_mut());
            for (name, rule) in fields.into_iter().flatten() {
                let flag = &stored.get("fields").unwrap_or(&serde_json::Value::Null)[name]["board"];
                if let (Some(rule), true) = (rule.as_object_mut(), flag.is_boolean()) {
                    rule.entry("board").or_insert_with(|| flag.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ac:iolhwin0
    #[test]
    fn a_sent_type_keeps_the_board_fields_that_it_omits() {
        let current =
            json!({"ac":{"title":"AC","color":"#63756b","badge":"AC","index":3,"title_size":14}});
        let mut command = json!({"op":"set_tree_schema","put_types":{
            "ac":{"title":"AC","question":"Q","color":"#112233","badge":null},
            "risk":{"title":"Risk"}}});
        keep_ui_fields(&mut command, &current);
        assert_eq!(
            command["put_types"],
            json!({"ac":{"title":"AC","question":"Q","color":"#112233","badge":null,"index":3,"title_size":14},
                "risk":{"title":"Risk"}})
        );
        let mut whole = json!({"op":"set_tree_schema","types":{"ac":{"title":"AC"}}});
        keep_ui_fields(&mut whole, &current);
        assert_eq!(whole["types"], current);
    }

    // ac:sigd1mxp
    #[test]
    fn a_sent_typed_field_keeps_its_board_flag_unless_it_sends_false() {
        let current = json!({"ac":{"title":"AC","fields":{
            "stage":{"type":"text","board":true},"env":{"type":"text","board":true},
            "note":{"type":"text"}}}});
        let mut command = json!({"op":"set_tree_schema","put_types":{"ac":{"title":"AC","fields":{
            "stage":{"type":"text"},"env":{"type":"text","board":false},
            "note":{"type":"text"},"risk":{"type":"text"}}}}});
        keep_ui_fields(&mut command, &current);
        assert_eq!(
            command["put_types"]["ac"]["fields"],
            json!({"stage":{"type":"text","board":true},"env":{"type":"text","board":false},
                "note":{"type":"text"},"risk":{"type":"text"}})
        );
    }
}
