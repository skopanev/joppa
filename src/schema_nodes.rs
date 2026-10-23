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
