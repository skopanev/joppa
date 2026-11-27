//! Part of `dependency_sql`: the walk of the `depends_on` graph. The edges come from one query,
//! so the walk needs no database and the tests run without one.
use std::collections::{BTreeMap, BTreeSet};

/// The state of one node of an edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub present: bool,
    pub satisfied: bool,
}

impl Node {
    /// A present node that is not satisfied: other work can wait on it.
    fn open(self) -> bool {
        self.present && !self.satisfied
    }
}

/// One `depends_on` edge: `source` waits on `target`. A missing node has no state.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub waiter: Option<Node>,
    pub target_node: Option<Node>,
}

/// The nodes that an unsatisfied target blocks directly, and the number of distinct dependents
/// of each open node with at least one. An archived source counts only with `archived`. A
/// missing target is satisfied. The count follows chains of open nodes only, so a satisfied
/// or archived node stops a chain.
pub fn walk(edges: &[Edge], archived: bool) -> (BTreeSet<String>, BTreeMap<String, i64>) {
    let blocks = |e: &&Edge| {
        e.target_node.is_some_and(|t| !t.satisfied)
            && e.waiter.is_some_and(|w| w.present || archived)
    };
    let direct = edges
        .iter()
        .filter(blocks)
        .map(|e| e.source.clone())
        .collect();
    // The waiters of each open target, through edges between two open nodes.
    let mut waiters: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in edges.iter().filter(|e| e.source != e.target) {
        if e.waiter.is_some_and(Node::open) && e.target_node.is_some_and(Node::open) {
            waiters.entry(&e.target).or_default().push(&e.source);
        }
    }
    let mut dependents = BTreeMap::new();
    for &target in waiters.keys() {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut next = vec![target];
        while let Some(node) = next.pop() {
            for &waiter in waiters.get(node).into_iter().flatten() {
                if waiter != target && seen.insert(waiter) {
                    next.push(waiter);
                }
            }
        }
        dependents.insert(target.to_owned(), seen.len() as i64);
    }
    (direct, dependents)
}
