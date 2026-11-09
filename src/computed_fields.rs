//! Computed data fields: a typed field of a tree type with `from` children, links or parent
//! (field_types.rs), or a multiple reference with `from` children. The status projection stores the values of each node in
//! `objects.computed`, and a read gives them in `data`.
//! - children: for an enum field, the value of the present children that comes first in
//!   `values`. For a list field or a reference with `from` children, the sorted union of the
//!   values of the present children, each value once.
//! - links: the own value of the present nodes with a depends_on or related edge to the node
//!   or from it, the one that comes first in `values`. A flow edge does not count.
//! - parent: the value of the nearest ancestor that has one.
//!
//! The order own, links, children, parent has no cycle: links reads only own values, children
//! skips the children of the parent rule, and parent reads the nodes above.
use crate::field_types::FieldType;
use crate::model::Workspace;
use crate::objects::{TreeType, schema};
use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
#[path = "computed_graph.rs"]
mod graph;
pub use graph::{Graph, Memo};

/// The data of a read: the stored data without the computed fields, then their values.
pub const DATA_SQL: &str = "(((o.document->'data')-ARRAY(SELECT jsonb_object_keys(o.computed)))||jsonb_strip_nulls(COALESCE(o.computed,'{}'::jsonb)))";

/// The clause of the computed fields of the data filter envelope `data`, a SQL parameter.
/// Each field matches the stored values or the data, so both GIN indexes find the
/// candidates. The data with the computed values then decides exactly.
pub fn filter_clause(data: &str, count: usize) -> String {
    if count == 0 {
        return String::new();
    }
    let each = (0..count).map(|i| {
        format!("(o.computed @> ({data}::text::jsonb->'each'->{i}) OR o.document->'data' @> ({data}::text::jsonb->'each'->{i}))")
    });
    let each: Vec<String> = each.collect();
    format!(
        " AND {} AND {} @> ({data}::text::jsonb->'computed')",
        each.join(" AND "),
        DATA_SQL
    )
}

/// The computed fields of a type: its typed fields with a source other than own, and its
/// references with `from`, which compute a list of addresses.
pub fn computed(t: &TreeType) -> BTreeMap<String, FieldType> {
    let fields = t.fields.iter().filter(|(_, r)| r.source() != "own");
    let references = t.references.iter().filter_map(|(field, r)| {
        let from = r.from.clone()?;
        let rule = FieldType {
            kind: "list".into(),
            values: vec![],
            from: Some(from),
        };
        Some((field.clone(), rule))
    });
    fields
        .map(|(field, r)| (field.clone(), r.clone()))
        .chain(references)
        .collect()
}

/// The rules of the computed fields: for each field, the types that compute it.
pub fn rules(types: &BTreeMap<String, TreeType>) -> BTreeMap<String, BTreeMap<String, FieldType>> {
    let mut out: BTreeMap<String, BTreeMap<String, FieldType>> = BTreeMap::new();
    for (kind, t) in types {
        for (field, rule) in computed(t) {
            out.entry(field).or_default().insert(kind.clone(), rule);
        }
    }
    out
}

/// Refuse a sent value of a computed field. Run it on the data of the command, before the
/// merge with the stored data.
pub fn guard(w: &Workspace, kind: &str, sent: &Value) -> Result<()> {
    let Some(t) = schema(w).types.get(kind).cloned() else {
        return Ok(());
    };
    for (field, rule) in computed(&t) {
        if sent.get(&field).is_some_and(|v| !v.is_null()) {
            let from = rule.source();
            let refusal = format!("{field} is computed from {from} and cannot be set on {kind}");
            bail!(crate::objects::RequiredField(refusal));
        }
    }
    Ok(())
}

/// Remove the computed fields of `kind` from the data that a save stores.
pub fn strip(w: &Workspace, kind: &str, data: &mut Value) {
    let Some(t) = schema(w).types.get(kind).cloned() else {
        return;
    };
    for field in computed(&t).keys() {
        data.as_object_mut().map(|m| m.remove(field));
    }
}
