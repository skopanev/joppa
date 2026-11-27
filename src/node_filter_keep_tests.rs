//! The kept set of the filters: keep and skip on every node, on a set and on the nodes
//! outside a set (node_filter_keep.rs), and the parse of tiers (node_filter_tiers.rs).
use super::*;
use crate::node_filter::NodeFilter;
use serde_json::json;

fn set(addresses: &[&str]) -> BTreeSet<String> {
    addresses.iter().map(|a| (*a).to_owned()).collect()
}

fn kept(addresses: &[&str], exclude: bool) -> Kept {
    Kept {
        set: Some(set(addresses)),
        exclude,
    }
}

// ac:zk5119jk
#[test]
fn keep_and_skip_combine_the_sets_of_the_filters() {
    let all = Kept::default();
    assert!(all.contains("a:1"));
    assert_eq!(all.clone().keep(set(&["a:1"])), kept(&["a:1"], false));
    assert_eq!(all.skip(set(&["a:1"])), kept(&["a:1"], true));
    let only = kept(&["a:1", "a:2"], false);
    assert_eq!(
        only.clone().keep(set(&["a:2", "a:3"])),
        kept(&["a:2"], false)
    );
    assert_eq!(only.skip(set(&["a:2"])), kept(&["a:1"], false));
    let outside = kept(&["a:1"], true);
    assert!(!outside.contains("a:1") && outside.contains("a:2"));
    let keep = outside.clone().keep(set(&["a:1", "a:2"]));
    assert_eq!(keep, kept(&["a:2"], false));
    assert_eq!(outside.skip(set(&["a:3"])), kept(&["a:1", "a:3"], true));
}

// ac:ow9ghazl
#[test]
fn tiers_take_1_to_10_filter_objects_without_nested_tiers() {
    let filter = NodeFilter::parse(&json!({"tiers": [{"status": "red"}, {}]})).unwrap();
    let tiers = filter.tiers.clone().unwrap();
    assert_eq!(tiers[0].statuses, Some(vec!["red".to_owned()]));
    assert_eq!(tiers[1], NodeFilter::default());
    // A read with tiers reads every level, also with only `{}`.
    assert!(filter.narrows());
    let query = NodeFilter::parse_query(&[("tiers", Some(r#"[{"status":"red"},{}]"#))]);
    assert_eq!(query.unwrap(), filter);
    let refused = |args: serde_json::Value| NodeFilter::parse(&args).unwrap_err().to_string();
    let count = "tiers takes 1 to 10 filter objects";
    assert_eq!(refused(json!({"tiers": []})), count);
    assert_eq!(refused(json!({"tiers": vec![json!({}); 11]})), count);
    assert_eq!(refused(json!({"tiers": {"status": "red"}})), count);
    assert_eq!(
        refused(json!({"tiers": ["red"]})),
        "each tier must be a filter object"
    );
    assert_eq!(
        refused(json!({"tiers": [{"tiers": [{}]}]})),
        "tiers cannot nest tiers"
    );
    // A tier takes the same filters as the read, with the same refusals.
    assert_eq!(
        refused(json!({"tiers": [{"depth": 0}]})),
        "depth must be an integer from 1 to 100"
    );
}
