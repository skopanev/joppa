//! The MCP catalog hides the admin and UI operations and refuses them as unknown operations.
use super::*;
use serde_json::json;

#[test]
fn the_mcp_catalog_is_the_full_catalog_without_the_hidden_operations() {
    let names: Vec<&str> = mcp_operations().map(|o| o.name).collect();
    assert_eq!(
        names,
        [
            "set_tree_schema",
            "save_object",
            "archive_object",
            "set_remote_links",
            "restore_object",
            "note",
            "withdraw_note",
            "detach",
        ]
    );
    for name in MCP_HIDDEN {
        assert!(operation(name).is_some(), "{name} left the full catalog");
    }
}

#[test]
fn a_hidden_operation_is_an_unknown_operation_and_a_batch_names_its_place() {
    let member = json!({"op":"member","id":"x","role":"member"});
    let note = json!({"op":"note","target":"ac:a","id":"n","text":"t"});
    let one = mcp_admit([&member], false).unwrap_err();
    assert_eq!(format!("{one:#}"), "unknown operation");
    let batch = mcp_admit([&note, &member], true).unwrap_err();
    assert_eq!(format!("{batch:#}"), "operation 2: unknown operation");
    assert!(mcp_admit([&note], true).is_ok());
    // The first operation outside the catalog stops the scan: admit refuses it.
    let removed = json!({"op":"revise"});
    assert!(mcp_admit([&removed, &member], true).is_ok());
}

#[test]
fn the_save_object_options_name_id_parent_clean_data_and_relations() {
    let (_, fields) = OPTIONAL_FIELDS
        .iter()
        .find(|(name, _)| *name == "save_object")
        .unwrap();
    let keys: Vec<&str> = fields.iter().map(|(key, _)| *key).collect();
    assert_eq!(keys, ["id", "parent", "clean", "data", "relations"]);
    assert!(fields.iter().all(|(_, text)| !text.trim().is_empty()));
    for (name, _) in OPTIONAL_FIELDS {
        assert!(!MCP_HIDDEN.contains(name), "{name} is hidden");
    }
}
