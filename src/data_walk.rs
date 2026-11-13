//! The walk of `POST /api/data`: a pre-order walk of the live tree in tree order. A node
//! that fails the filter is skipped with its whole branch. The type selection picks the
//! nodes of the reply and does not stop the walk.
use crate::model::Workspace;
use crate::objects::TreeType;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// One live node of the walk. `data` holds the computed fields too.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub address: String,
    pub kind: String,
    pub id: String,
    pub parent: Option<String>,
    pub data: Value,
}

/// True when the node passes each filter field (see `field_matches`).
pub fn matches(data: &Value, filter: &[(String, Vec<Value>)]) -> bool {
    filter
        .iter()
        .all(|(field, wanted)| field_matches(data.get(field), wanted))
}

/// A node value passes when it is absent, null, "" or [], when either side is or holds "*",
/// when a scalar equals a wanted value, or when a list shares a value with `wanted`.
pub fn field_matches(value: Option<&Value>, wanted: &[Value]) -> bool {
    let star = Value::from("*");
    let any = |v: &Value| *v == star || wanted.iter().any(|w| same(v, w));
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) if s.is_empty() => true,
        Some(Value::Array(items)) if items.is_empty() => true,
        Some(_) if wanted.contains(&star) => true,
        Some(Value::Array(items)) => items.iter().any(any),
        Some(value) => any(value),
    }
}

/// JSON equality, with numbers compared by value, so 3 equals 3.0.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// The live nodes of one read, with the children of each parent.
pub struct Tree<'a> {
    pub w: &'a Workspace,
    pub types: &'a BTreeMap<String, TreeType>,
    pub nodes: BTreeMap<String, Node>,
    children: BTreeMap<Option<String>, Vec<String>>,
}

impl<'a> Tree<'a> {
    pub fn new(
        w: &'a Workspace,
        types: &'a BTreeMap<String, TreeType>,
        nodes: impl IntoIterator<Item = Node>,
    ) -> Self {
        let nodes: BTreeMap<String, Node> =
            nodes.into_iter().map(|n| (n.address.clone(), n)).collect();
        let mut children: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
        for node in nodes.values() {
            let parent = node.parent.clone().filter(|p| nodes.contains_key(p));
            if parent.is_some() || node.parent.is_none() {
                children
                    .entry(parent)
                    .or_default()
                    .push(node.address.clone());
            }
        }
        Tree {
            w,
            types,
            nodes,
            children,
        }
    }

    /// The children of `parent` (the roots for None) in tree order: the type index, then the
    /// stored order of the siblings of one type, then the ID.
    pub fn ordered(&self, parent: Option<&str>) -> Vec<String> {
        let mut groups: BTreeMap<(u32, &str), BTreeMap<&str, &str>> = BTreeMap::new();
        let listed = self.children.get(&parent.map(str::to_owned));
        for address in listed.into_iter().flatten() {
            let node = &self.nodes[address];
            let index = self.types.get(&node.kind).and_then(|t| t.index);
            let group = groups.entry((index.unwrap_or(u32::MAX), &node.kind));
            group.or_default().insert(&node.id, address);
        }
        let mut out = vec![];
        for ((_, kind), ids) in groups {
            let members = ids.keys().map(|id| id.to_string()).collect();
            for id in crate::tree_order::arrange(self.w, kind, parent, members) {
                out.push(ids[id.as_str()].to_owned());
            }
        }
        out
    }

    /// The addresses of the reply in walk order, from `start`: the root, or the roots.
    pub fn walk(
        &self,
        start: Option<&str>,
        filter: &[(String, Vec<Value>)],
        kinds: Option<&BTreeSet<String>>,
    ) -> Vec<String> {
        let mut out = vec![];
        let mut stack: Vec<String> = match start {
            Some(root) => vec![root.to_owned()],
            None => self.ordered(None),
        };
        stack.reverse();
        while let Some(address) = stack.pop() {
            let Some(node) = self.nodes.get(&address) else {
                continue;
            };
            if !matches(&node.data, filter) {
                continue;
            }
            if kinds.is_none_or(|k| k.contains(&node.kind)) {
                out.push(address.clone());
            }
            stack.extend(self.ordered(Some(&address)).into_iter().rev());
        }
        out
    }
}

#[cfg(test)]
#[path = "data_walk_tests.rs"]
mod tests;
