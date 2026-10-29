use super::*;

/// Capability C holds A with criteria A1 and A2, and B with criterion B1. B waits for A.
fn tree() -> Graph {
    let mut g = Graph::default();
    g.node("capability:c".into(), None, false);
    for (key, parent, criterion) in [
        ("requirement:a", "capability:c", false),
        ("requirement:b", "capability:c", false),
        ("ac:a1", "requirement:a", true),
        ("ac:a2", "requirement:a", true),
        ("ac:b1", "requirement:b", true),
    ] {
        g.node(key.into(), Some(parent.into()), criterion);
    }
    g.link("requirement:b".into(), "requirement:a".into());
    g
}
fn green(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| (*k).to_owned()).collect()
}

// dependencies-before-priority-ac5
#[test]
fn a_wait_counts_each_unsatisfied_target_of_the_node_and_its_ancestors() {
    let mut g = tree();
    g.node("requirement:d".into(), Some("capability:c".into()), false);
    g.node("ac:d1".into(), Some("requirement:d".into()), true);
    g.link("requirement:b".into(), "requirement:d".into());
    g.link("ac:b1".into(), "requirement:a".into());
    let counts = g.wait_counts(&green(&[]));
    // B waits for A and D. B1 counts A once, from its own link and from B, and D.
    assert_eq!(counts.get("requirement:b"), Some(&2));
    assert_eq!(counts.get("ac:b1"), Some(&2));
    // A green target leaves the count.
    assert_eq!(
        g.wait_counts(&green(&["ac:d1"])).get("requirement:b"),
        Some(&1)
    );
    assert!(
        g.wait_counts(&green(&["ac:a1", "ac:a2", "ac:d1"]))
            .is_empty()
    );
    assert_eq!(counts.get("requirement:a"), None);
}

#[test]
fn work_below_a_link_waits_until_every_target_criterion_is_green() {
    let g = tree();
    assert_eq!(g.criteria_needed(), ["ac:a1", "ac:a2"]);
    let waits = g.waits(&green(&["ac:a1"]));
    assert_eq!(
        waits.get("ac:b1").map(String::as_str),
        Some("requirement:a")
    );
    assert_eq!(
        waits.get("requirement:b").map(String::as_str),
        Some("requirement:a")
    );
    assert!(!waits.contains_key("ac:a1") && !waits.contains_key("capability:c"));
    assert!(g.waits(&green(&["ac:a1", "ac:a2"])).is_empty());
}

#[test]
fn a_target_without_live_criteria_holds_work_and_an_archived_target_does_not() {
    let mut g = tree();
    g.parents.remove("ac:a1");
    g.parents.remove("ac:a2");
    g.criteria.clear();
    assert_eq!(
        g.waits(&green(&[])).get("ac:b1").map(String::as_str),
        Some("requirement:a")
    );
    g.parents.remove("requirement:a");
    assert!(g.waits(&green(&[])).is_empty());
}

#[test]
fn a_shared_parent_alone_never_makes_work_wait() {
    let mut g = tree();
    g.links.clear();
    assert!(g.criteria_needed().is_empty());
    assert!(g.waits(&green(&[])).is_empty());
}
