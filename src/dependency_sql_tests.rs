//! The dependency facts of the items, of the order parameter and of the graph walk
//! (dependency_sql.rs, dependency_graph.rs).
use super::*;

fn facts() -> Facts {
    Facts {
        blocked: ["ac:a".to_owned()].into(),
        dependents: [("ac:c".to_owned(), 2), ("ac:b".to_owned(), 1)].into(),
    }
}

const OPEN: Option<Node> = Some(Node {
    present: true,
    satisfied: false,
});
const DONE: Option<Node> = Some(Node {
    present: true,
    satisfied: true,
});
const ARCHIVED: Option<Node> = Some(Node {
    present: false,
    satisfied: false,
});

fn edge(source: &str, target: &str, waiter: Option<Node>, target_node: Option<Node>) -> Edge {
    let (source, target) = (source.to_owned(), target.to_owned());
    Edge {
        source,
        target,
        waiter,
        target_node,
    }
}

#[test]
fn decorate_gives_explicit_values() {
    let mut items = vec![
        json!({"address": "ac:a"}),
        json!({"address": "ac:c"}),
        json!({}),
    ];
    facts().decorate(&mut items);
    let first = json!({"address": "ac:a", "blocked": true, "dependents": 0});
    assert_eq!(items[0], first);
    let second = json!({"address": "ac:c", "blocked": false, "dependents": 2});
    assert_eq!(items[1], second);
    assert_eq!(items[2], json!({}));
}

#[test]
fn json_names_each_node_once() {
    let text = facts().json();
    let map: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(map["ac:a"], json!({"blocked": true, "dependents": 0}));
    assert_eq!(map["ac:b"], json!({"blocked": false, "dependents": 1}));
    assert_eq!(map.as_object().unwrap().len(), 3);
    assert_eq!(Facts::default().json(), "{}");
}

#[test]
fn a_chain_counts_each_waiter_once() {
    // a waits on b, b waits on c, and d waits on c too.
    let edges = [
        edge("a", "b", OPEN, OPEN),
        edge("b", "c", OPEN, OPEN),
        edge("d", "c", OPEN, OPEN),
    ];
    let (direct, dependents) = walk(&edges, false);
    assert_eq!(direct, ["a", "b", "d"].map(String::from).into());
    let counts: BTreeMap<String, i64> = [("b".into(), 1), ("c".into(), 3)].into();
    assert_eq!(dependents, counts);
}

#[test]
fn a_satisfied_or_missing_target_blocks_nothing() {
    let edges = [
        edge("a", "b", OPEN, DONE),
        edge("b", "c", DONE, OPEN),
        edge("x", "gone", OPEN, None),
    ];
    let (direct, dependents) = walk(&edges, false);
    // A satisfied waiter can still wait on an open target, but it is no dependent.
    assert_eq!(direct, ["b".to_owned()].into());
    assert!(dependents.is_empty());
}

#[test]
fn an_archived_node_stops_a_chain() {
    let edges = [
        edge("a", "b", OPEN, ARCHIVED),
        edge("z", "a", ARCHIVED, OPEN),
    ];
    let (direct, dependents) = walk(&edges, false);
    // An unsatisfied archived target blocks, and an archived waiter counts only with archived.
    assert_eq!(direct, ["a".to_owned()].into());
    assert!(dependents.is_empty());
    assert_eq!(walk(&edges, true).0, ["a", "z"].map(String::from).into());
}

#[test]
fn a_cycle_ends() {
    let edges = [
        edge("a", "b", OPEN, OPEN),
        edge("b", "a", OPEN, OPEN),
        edge("a", "a", OPEN, OPEN),
    ];
    let (_, dependents) = walk(&edges, false);
    let counts: BTreeMap<String, i64> = [("a".into(), 1), ("b".into(), 1)].into();
    assert_eq!(dependents, counts);
}

#[test]
fn the_query_reads_the_rules_of_the_schema() {
    // The status rule and the done rule come from the board projection, not from type names.
    assert!(EDGES.contains("first_view->'object_types'"));
    assert!(EDGES.contains("k.value->'done'"));
    assert!(!EDGES.contains("RECURSIVE"));
    assert!(!EDGES.contains("'task'") && !EDGES.contains("'stage'"));
}
