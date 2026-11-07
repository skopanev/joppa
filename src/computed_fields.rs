//! Computed data fields: a typed field of a tree type with `from` children, links or parent
//! (field_types.rs). The status projection stores the values of each node in
//! `objects.computed`, and a read gives them in `data`.
//! - children: the value of the present children that comes first in `values`.
//! - links: the own value of the present nodes that link to the node or that it links to,
//!   the one that comes first in `values`.
//! - parent: the value of the nearest ancestor that has one.
//!
//! The order own, links, children, parent has no cycle: links reads only own values, children
//! skips the children of the parent rule, and parent reads the nodes above.
use crate::field_types::FieldType;
use crate::model::Workspace;
use crate::objects::{TreeType, resolve_address, schema};
use anyhow::{Result, bail};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The data of a read: the stored data without the computed fields, then their values.
pub const DATA_SQL: &str = "(((o.document->'data')-ARRAY(SELECT jsonb_object_keys(o.computed)))||jsonb_strip_nulls(COALESCE(o.computed,'{}'::jsonb)))";

/// The rules of the computed fields: for each field, the types that compute it.
pub fn rules(types: &BTreeMap<String, TreeType>) -> BTreeMap<String, BTreeMap<String, FieldType>> {
    let mut out: BTreeMap<String, BTreeMap<String, FieldType>> = BTreeMap::new();
    for (kind, t) in types {
        for (field, rule) in t.fields.iter().filter(|(_, r)| r.source() != "own") {
            out.entry(field.clone())
                .or_default()
                .insert(kind.clone(), rule.clone());
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
    for (field, rule) in t.fields.iter().filter(|(_, r)| r.source() != "own") {
        if sent.get(field).is_some_and(|v| !v.is_null()) {
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
    for (field, _) in t.fields.iter().filter(|(_, r)| r.source() != "own") {
        data.as_object_mut().map(|m| m.remove(field));
    }
}

/// The present nodes of a workspace with their children and their links in both directions.
pub struct Graph<'a> {
    w: &'a Workspace,
    types: BTreeMap<String, TreeType>,
    children: BTreeMap<String, Vec<String>>,
    pub linked: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> Graph<'a> {
    pub fn new(w: &'a Workspace) -> Self {
        let mut graph = Graph {
            w,
            types: schema(w).types,
            children: BTreeMap::new(),
            linked: BTreeMap::new(),
        };
        for (address, o) in w.objects.iter().filter(|(_, o)| o.current().present) {
            let current = o.current();
            if let Some(parent) = &current.parent {
                let parent = resolve_address(w, parent).to_owned();
                graph
                    .children
                    .entry(parent)
                    .or_default()
                    .push(address.clone());
            }
            for link in &current.links {
                let target = resolve_address(w, &link.target).to_owned();
                if graph.present(&target) && target != *address {
                    graph
                        .linked
                        .entry(address.clone())
                        .or_default()
                        .insert(target.clone());
                    graph
                        .linked
                        .entry(target)
                        .or_default()
                        .insert(address.clone());
                }
            }
        }
        graph
    }

    fn present(&self, address: &str) -> bool {
        self.w
            .objects
            .get(address)
            .is_some_and(|o| o.current().present)
    }

    fn rule(&self, address: &str, field: &str) -> Option<&FieldType> {
        let kind = &self.w.objects.get(address)?.kind;
        self.types.get(kind)?.fields.get(field)
    }

    fn source(&self, address: &str, field: &str) -> &str {
        self.rule(address, field).map_or("own", FieldType::source)
    }

    fn own(&self, address: &str, field: &str) -> Option<Value> {
        let value = self.w.objects.get(address)?.current().data.get(field)?;
        (!value.is_null()).then(|| value.clone())
    }

    /// The parent of a present node.
    pub fn parent(&self, address: &str) -> Option<String> {
        let o = self.w.objects.get(address)?;
        let parent = o.current().parent.as_deref()?;
        Some(resolve_address(self.w, parent).to_owned())
    }

    /// The present children of a node.
    pub fn children(&self, address: &str) -> &[String] {
        self.children.get(address).map_or(&[], Vec::as_slice)
    }

    /// The value of `field` of a present node. `memo` keeps the values of this projection.
    pub fn value(&self, field: &str, address: &str, memo: &mut Memo) -> Option<Value> {
        let key = (field.to_owned(), address.to_owned());
        if let Some(value) = memo.get(&key) {
            return value.clone();
        }
        let value = self.compute(field, address, memo);
        memo.insert(key, value.clone());
        value
    }

    fn compute(&self, field: &str, address: &str, memo: &mut Memo) -> Option<Value> {
        if !self.present(address) {
            return None;
        }
        let Some(rule) = self.rule(address, field).filter(|r| r.source() != "own") else {
            return self.own(address, field);
        };
        match rule.source() {
            "links" => {
                let linked = self.linked.get(address).into_iter().flatten();
                let own = linked.filter(|a| self.source(a, field) == "own");
                first(&rule.values, own.filter_map(|a| self.own(a, field)))
            }
            "children" => {
                let mut values = vec![];
                for child in self.children(address) {
                    if self.source(child, field) != "parent" {
                        values.extend(self.value(field, child, memo));
                    }
                }
                first(&rule.values, values.into_iter())
            }
            _ => {
                let mut seen = BTreeSet::from([address.to_owned()]);
                let mut above = self.parent(address);
                while let Some(node) = above.filter(|a| seen.insert(a.clone())) {
                    if self.source(&node, field) != "parent"
                        && let Some(value) = self.value(field, &node, memo)
                    {
                        return Some(value);
                    }
                    above = self.parent(&node);
                }
                None
            }
        }
    }

    /// The stored value of a node: each computed field of its type with its value or null.
    /// A node whose type computes no field has none.
    pub fn row(&self, address: &str, memo: &mut Memo) -> Option<Value> {
        let kind = &self.w.objects.get(address)?.kind;
        let t = self.types.get(kind)?;
        let fields: Vec<&String> = t
            .fields
            .iter()
            .filter(|(_, r)| r.source() != "own")
            .map(|(f, _)| f)
            .collect();
        if fields.is_empty() || !self.present(address) {
            return None;
        }
        let mut row = Map::new();
        for field in fields {
            let value = self.value(field, address, memo).unwrap_or(Value::Null);
            row.insert(field.clone(), value);
        }
        Some(Value::Object(row))
    }
}

/// The values that one projection computed: (field, address) to value.
pub type Memo = BTreeMap<(String, String), Option<Value>>;

/// The value that comes first in `values`. A value that `values` does not name counts not.
fn first(values: &[String], found: impl Iterator<Item = Value>) -> Option<Value> {
    found
        .filter_map(|v| {
            let text = v.as_str()?;
            Some((values.iter().position(|x| x == text)?, v.clone()))
        })
        .min_by_key(|(index, _)| *index)
        .map(|(_, v)| v)
}
