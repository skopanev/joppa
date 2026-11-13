use super::*;
use serde_json::json;

fn yes(value: Value, wanted: Value) -> bool {
    let wanted = match wanted {
        Value::Array(items) => items,
        one => vec![one],
    };
    field_matches(Some(&value), &wanted)
}

#[test]
fn scalar_values_match_one_wanted_value_or_a_list() {
    assert!(yes(json!("x"), json!("x")));
    assert!(yes(json!("x"), json!(["y", "x"])));
    assert!(!yes(json!("x"), json!("y")));
    assert!(yes(json!(3), json!(3)));
    assert!(yes(json!(3), json!([1, 3.0])));
    assert!(!yes(json!(3), json!(4)));
    assert!(yes(json!(true), json!([false, true])));
    assert!(!yes(json!(true), json!(false)));
}

#[test]
fn large_integers_compare_exactly() {
    assert!(!yes(
        json!(9007199254740992_i64),
        json!(9007199254740993_i64)
    ));
    assert!(!yes(
        json!(18446744073709551614_u64),
        json!(18446744073709551615_u64)
    ));
    assert!(yes(
        json!(18446744073709551615_u64),
        json!([1, 18446744073709551615_u64])
    ));
    assert!(!yes(json!(-1), json!(18446744073709551615_u64)));
}

#[test]
fn a_star_on_either_side_matches_any_value() {
    assert!(yes(json!("x"), json!("*")));
    assert!(yes(json!("*"), json!("y")));
    assert!(yes(json!(["*"]), json!(["ui"])));
    assert!(yes(json!(["api"]), json!(["*"])));
}

#[test]
fn a_list_value_matches_when_it_shares_a_wanted_value() {
    assert!(yes(json!(["api", "db"]), json!("db")));
    assert!(yes(json!(["api", "db"]), json!(["ui", "api"])));
    assert!(!yes(json!(["api", "db"]), json!(["ui"])));
}

#[test]
fn an_empty_value_matches_each_filter() {
    let wanted = [json!("prod")];
    assert!(field_matches(None, &wanted));
    for empty in [json!(null), json!(""), json!([])] {
        assert!(field_matches(Some(&empty), &wanted), "{empty}");
    }
    let filter = vec![("env".to_owned(), wanted.to_vec())];
    assert!(matches(&json!({"other": 1}), &filter));
    assert!(!matches(&json!({"env": "dev"}), &filter));
}

fn node(address: &str, parent: Option<&str>, data: Value) -> Node {
    let (kind, id) = address.split_once(':').unwrap();
    Node {
        address: address.into(),
        kind: kind.into(),
        id: id.into(),
        parent: parent.map(str::to_owned),
        data,
    }
}

#[test]
fn the_walk_prunes_failed_branches_and_keeps_tree_order() {
    let types: BTreeMap<String, TreeType> = serde_json::from_value(json!({
        "box": {"title": "Box", "role": "group", "index": 0},
        "type_a": {"title": "A", "role": "document", "index": 1},
        "type_b": {"title": "B", "role": "document", "index": 2}
    }))
    .unwrap();
    let mut w = Workspace::default();
    let order = ["a3", "a1", "a2"].map(str::to_owned).to_vec();
    w.tree_order.insert("type_a:box:root".into(), order);
    let nodes = [
        node("box:root", None, json!({})),
        node("type_b:b0", Some("box:root"), json!({})),
        node("type_a:a1", Some("box:root"), json!({"env": "prod"})),
        node("type_a:a2", Some("box:root"), json!({"env": "dev"})),
        node("type_a:a3", Some("box:root"), json!({})),
        node("type_b:b1", Some("type_a:a1"), json!({"env": "prod"})),
        node("type_b:b2", Some("type_a:a2"), json!({"env": "prod"})),
        node("type_b:b3", Some("type_a:a3"), json!({"env": ""})),
        node("box:other", None, json!({})),
        node("type_a:c1", Some("box:other"), json!({"env": "prod"})),
    ];
    let tree = Tree::new(&w, &types, nodes);
    let filter = vec![("env".to_owned(), vec![json!("prod")])];
    let both: BTreeSet<String> = ["type_a".into(), "type_b".into()].into();
    // The type index puts type_a before type_b under one parent.
    let walked = tree.walk(Some("box:root"), &filter, Some(&both));
    let expected = [
        "type_a:a3",
        "type_b:b3",
        "type_a:a1",
        "type_b:b1",
        "type_b:b0",
    ];
    assert_eq!(walked, expected);
    let only_a: BTreeSet<String> = ["type_a".into()].into();
    // Roots without a stored order come in ID order.
    let walked = tree.walk(None, &filter, Some(&only_a));
    assert_eq!(walked, ["type_a:c1", "type_a:a3", "type_a:a1"]);
    assert!(tree.walk(Some("type_a:a2"), &filter, None).is_empty());
    assert_eq!(
        tree.walk(Some("type_a:a1"), &[], None),
        ["type_a:a1", "type_b:b1"]
    );
}
