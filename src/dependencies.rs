//! Ordering dependencies. Work waits while a blocking link of its node, or of an ancestor,
//! has a live target that is not satisfied. The same rule holds for every type: a live target
//! is satisfied when it is a green criterion, or when it has live criteria below it and each
//! of them is green. An archived target does not make work wait.
//! Reads and live writes compute the wait; projections and replay never store it.
use crate::model::Workspace;
use std::collections::{BTreeMap, BTreeSet};

/// The live tree with the blocking links of every node.
#[derive(Default)]
pub struct Graph {
    parents: BTreeMap<String, Option<String>>,
    criteria: BTreeSet<String>,
    links: BTreeMap<String, BTreeSet<String>>,
}

impl Graph {
    pub fn from_state(w: &Workspace) -> Self {
        let types = crate::objects::schema(w).types;
        let resolve = |key: &str| crate::objects::resolve_address(w, key).to_owned();
        let mut graph = Self::default();
        for (key, o) in w.objects.iter().filter(|(_, o)| o.current().present) {
            let role = types.get(&o.kind).map(|t| t.role.as_str());
            graph.node(
                key.clone(),
                o.current().parent.as_deref().map(resolve),
                role == Some("ac"),
            );
            for link in &o.current().links {
                if link.blocking && link.relation == "depends_on" {
                    graph.link(key.clone(), resolve(&link.target));
                }
            }
        }
        graph
    }
    /// A live node with its parent.
    pub fn node(&mut self, key: String, parent: Option<String>, criterion: bool) {
        if criterion {
            self.criteria.insert(key.clone());
        }
        self.parents.insert(key, parent);
    }
    /// A blocking ordering link of a live node.
    pub fn link(&mut self, source: String, target: String) {
        self.links.entry(source).or_default().insert(target);
    }
    pub fn has_links(&self) -> bool {
        !self.links.is_empty()
    }
    /// The live criteria that decide each live link target: the target itself, or each one
    /// below it. A link to an archived node does not make work wait, so it has no entry.
    fn deciding(&self) -> BTreeMap<&str, Vec<&str>> {
        let mut children = BTreeMap::<&str, Vec<&str>>::new();
        for (child, parent) in &self.parents {
            if let Some(parent) = parent {
                children.entry(parent).or_default().push(child);
            }
        }
        let mut deciding = BTreeMap::new();
        for target in self.links.values().flatten() {
            if !self.parents.contains_key(target) || deciding.contains_key(target.as_str()) {
                continue;
            }
            let mut found = vec![];
            if self.criteria.contains(target) {
                found.push(target.as_str());
            } else {
                let mut stack = vec![target.as_str()];
                let mut seen = BTreeSet::new();
                while let Some(node) = stack.pop() {
                    for child in children.get(node).into_iter().flatten() {
                        if seen.insert(*child) {
                            if self.criteria.contains(*child) {
                                found.push(*child);
                            }
                            stack.push(*child);
                        }
                    }
                }
            }
            deciding.insert(target.as_str(), found);
        }
        deciding
    }
    /// The criteria whose test status the wait needs.
    pub fn criteria_needed(&self) -> Vec<String> {
        let needed: BTreeSet<&str> = self.deciding().into_values().flatten().collect();
        needed.into_iter().map(str::to_owned).collect()
    }
    /// The live link targets that are not satisfied: a target without live criteria, or with
    /// a criterion that is not green.
    fn unsatisfied(&self, green: &BTreeSet<String>) -> BTreeSet<&str> {
        self.deciding()
            .into_iter()
            .filter(|(_, deciding)| {
                deciding.is_empty() || deciding.iter().any(|c| !green.contains(*c))
            })
            .map(|(target, _)| target)
            .collect()
    }
    /// The number of distinct unsatisfied targets of each waiting live node, over the
    /// links of the node and of each ancestor up the tree.
    pub fn wait_counts(&self, green: &BTreeSet<String>) -> BTreeMap<String, usize> {
        let unsatisfied = self.unsatisfied(green);
        let mut counts = BTreeMap::new();
        if unsatisfied.is_empty() {
            return counts;
        }
        for node in self.parents.keys() {
            let mut targets = BTreeSet::new();
            let mut current = Some(node.as_str());
            let mut seen = BTreeSet::new();
            while let Some(key) = current.filter(|k| seen.insert(*k)) {
                for target in self.links.get(key).into_iter().flatten() {
                    if unsatisfied.contains(target.as_str()) {
                        targets.insert(target.as_str());
                    }
                }
                current = self.parents.get(key).and_then(|p| p.as_deref());
            }
            if !targets.is_empty() {
                counts.insert(node.clone(), targets.len());
            }
        }
        counts
    }
    /// The first unsatisfied target of each waiting live node: the node's own links in
    /// address order first, then the links of each ancestor up the tree.
    pub fn waits(&self, green: &BTreeSet<String>) -> BTreeMap<String, String> {
        let unsatisfied = self.unsatisfied(green);
        let first: BTreeMap<&str, &str> = self
            .links
            .iter()
            .filter_map(|(source, targets)| {
                let target = targets.iter().find(|t| unsatisfied.contains(t.as_str()))?;
                Some((source.as_str(), target.as_str()))
            })
            .collect();
        let mut waits = BTreeMap::new();
        if first.is_empty() {
            return waits;
        }
        for node in self.parents.keys() {
            let mut current = Some(node.as_str());
            let mut seen = BTreeSet::new();
            while let Some(key) = current.filter(|k| seen.insert(*k)) {
                if let Some(target) = first.get(key) {
                    waits.insert(node.clone(), (*target).to_owned());
                    break;
                }
                current = self.parents.get(key).and_then(|p| p.as_deref());
            }
        }
        waits
    }
}

/// The refusal and the waiting reason of work that waits for `target`.
pub fn reason(target: &str) -> String {
    format!("waits for {target}, which is not green")
}

#[cfg(test)]
#[path = "dependencies_tests.rs"]
mod tests;
