//! Part of computed_fields.rs: the graph of the present nodes and the computation of the
//! value of one computed field of one node.
use super::computed;
use crate::field_types::FieldType;
use crate::model::Workspace;
use crate::objects::{TreeType, resolve_address, schema};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The present nodes of a workspace with their children and their links in both directions.
pub struct Graph<'a> {
    w: &'a Workspace,
    types: BTreeMap<String, TreeType>,
    /// The computed fields of each type.
    rules: BTreeMap<String, BTreeMap<String, FieldType>>,
    children: BTreeMap<String, Vec<String>>,
    pub linked: BTreeMap<String, BTreeSet<String>>,
    /// The present nodes that link to each absent node, such as an archived one.
    pub linked_absent: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> Graph<'a> {
    pub fn new(w: &'a Workspace) -> Self {
        let types = schema(w).types;
        let rules = types
            .iter()
            .map(|(k, t)| (k.clone(), computed(t)))
            .collect();
        let mut graph = Graph {
            w,
            types,
            rules,
            children: BTreeMap::new(),
            linked: BTreeMap::new(),
            linked_absent: BTreeMap::new(),
        };
        for (address, o) in w.objects.iter().filter(|(_, o)| o.current().present) {
            let current = o.current();
            if let Some(parent) = &current.parent {
                let parent = resolve_address(w, parent).to_owned();
                let children = graph.children.entry(parent).or_default();
                children.push(address.clone());
            }
            for edge in current.counted() {
                let target = resolve_address(w, &edge.to).to_owned();
                if graph.present(&target) && target != *address {
                    let from = graph.linked.entry(address.clone()).or_default();
                    from.insert(target.clone());
                    let to = graph.linked.entry(target).or_default();
                    to.insert(address.clone());
                } else if target != *address {
                    let to = graph.linked_absent.entry(target).or_default();
                    to.insert(address.clone());
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

    /// The computed fields of the type of a node.
    pub fn fields(&self, address: &str) -> Option<&BTreeMap<String, FieldType>> {
        let kind = &self.w.objects.get(address)?.kind;
        self.rules.get(kind).filter(|rules| !rules.is_empty())
    }

    fn rule(&self, address: &str, field: &str) -> Option<&FieldType> {
        self.fields(address)?.get(field)
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
        let Some(rule) = self.rule(address, field) else {
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
                    if self.source(child, field) != "parent"
                        && let Some(value) = self.value(field, child, memo)
                    {
                        values.push((child, value));
                    }
                }
                if rule.kind == "enum" {
                    return first(&rule.values, values.into_iter().map(|(_, v)| v));
                }
                let mut union = BTreeSet::new();
                for (child, value) in values {
                    union.extend(self.texts(child, field, &value));
                }
                union.retain(|v| rule.values.is_empty() || rule.values.contains(v));
                (!union.is_empty()).then(|| Value::from(Vec::from_iter(union)))
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

    /// The texts of the value of a child: a text or a list of texts. When the type of the
    /// child has a reference rule for the field, each text becomes a full address.
    fn texts(&self, child: &str, field: &str, value: &Value) -> Vec<String> {
        let texts: Vec<&str> = match value {
            Value::String(text) => vec![text],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let kind = self.w.objects.get(child).map(|o| o.kind.as_str());
        let reference = kind.and_then(|k| self.types.get(k)?.references.get(field));
        let Some(reference) = reference else {
            return texts.into_iter().map(str::to_owned).collect();
        };
        texts
            .into_iter()
            .filter(|text| !(reference.allow_all && *text == "all"))
            .map(|text| match text.contains(':') || reference.target == "*" {
                true => resolve_address(self.w, text).to_owned(),
                false => {
                    let address = format!("{}:{text}", reference.target);
                    resolve_address(self.w, &address).to_owned()
                }
            })
            .collect()
    }

    /// The stored value of a node: each computed field of its type with its value or null.
    /// A node whose type computes no field has none.
    pub fn row(&self, address: &str, memo: &mut Memo) -> Option<Value> {
        let fields = self.fields(address)?;
        if !self.present(address) {
            return None;
        }
        let mut row = Map::new();
        for field in fields.keys() {
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
