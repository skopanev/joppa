//! The MCP catalog: the operations that MCP shows, their optional fields, and the refusal of
//! an operation that MCP hides. `POST /api/workspaces/{w}/commands` and the journal replay keep
//! the full catalog (requirement:help-matches-api).
use super::*;
use serde_json::Value;

/// The admin and UI operations. MCP hides them: joppa_help does not name them, the op enum of
/// joppa_command omits them, and an MCP call of one is an unknown operation. joppa_attach sends
/// its own attach command past this gate.
pub const MCP_HIDDEN: &[&str] = &[
    "reorder_tree",
    "attach",
    "redact_object",
    "member",
    "remove_member",
    "update_workspace",
    "delete_workspace",
];

/// The operations of the MCP catalog, in the order of `OPERATIONS`.
pub fn mcp_operations() -> impl Iterator<Item = &'static Operation> {
    OPERATIONS.iter().filter(|o| !MCP_HIDDEN.contains(&o.name))
}

/// Refuse the first MCP command whose op MCP hides, before any write. The scan stops at the
/// first op outside the catalog: `admit` (store_write_item_ids.rs) gives its refusal, so the
/// first bad operation of a batch names the error.
pub fn mcp_admit<'a>(commands: impl IntoIterator<Item = &'a Value>, batch: bool) -> Result<()> {
    for (index, command) in commands.into_iter().enumerate() {
        let Some(name) = command["op"].as_str().filter(|n| operation(n).is_some()) else {
            return Ok(());
        };
        if MCP_HIDDEN.contains(&name) {
            let refusal = anyhow::anyhow!("unknown operation");
            return Err(if batch {
                refusal.context(format!("operation {}", index + 1))
            } else {
                refusal
            });
        }
    }
    Ok(())
}

/// The optional fields of an MCP operation with their allowed values, as help gives them.
pub const OPTIONAL_FIELDS: &[(&str, &[(&str, &str)])] = &[
    (
        "save_object",
        &[
            (
                "id",
                "a new object omits it, and the server makes one; an edit keeps the id it read",
            ),
            (
                "parent",
                "the parent address, or null for a root; an update without parent keeps the parent",
            ),
            (
                "clean",
                "true replaces data whole; only with revision above 0",
            ),
            (
                "data",
                "an update merges data by key, and null removes a field; optional on an update \
                 without clean",
            ),
            (
                "relations",
                "{key: {title?, color?, facts?, edges: [{to, when?, revision?, color?}]}} by the \
                 keys of schema.relations: a sent key replaces its title, edges, color and facts, \
                 null removes it, other keys stay; color is a palette name or #rrggbb, facts are \
                 objects; depends_on refuses a cycle, related and a flow block nothing",
            ),
        ],
    ),
    (
        "set_tree_schema",
        &[
            (
                "put_relations",
                "{key: {title, description?}}: adds a relation or replaces its texts",
            ),
            (
                "remove_relations",
                "[key]: removes a relation that no live node holds; not depends_on or related",
            ),
        ],
    ),
];

#[cfg(test)]
#[path = "commands_mcp_tests.rs"]
mod tests;
