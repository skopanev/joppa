//! The data set filters: their shapes, their probes and their SQL (node_filter_sets.rs).
use super::*;
use serde_json::json;

fn path(key: &str) -> Vec<String> {
    key.split('.').map(str::to_owned).collect()
}

// ac:zk5119jk
#[test]
fn a_key_takes_a_string_or_a_list_and_a_dotted_key_names_a_nested_field() {
    let args = json!({"data_any": {"tags": ["a", "b"], "delivery.state": "late"},
        "data_none": {}});
    let sets = DataSets::parse(&args).unwrap();
    let any = vec![
        (path("delivery.state"), vec!["late".to_owned()]),
        (path("tags"), vec!["a".to_owned(), "b".to_owned()]),
    ];
    assert_eq!(sets.any, Some(any));
    assert_eq!(sets.all, None);
    // An empty object is set, so it narrows the read and refuses an address.
    assert_eq!(sets.none, Some(vec![]));
    assert!(sets.is_set());
    // An HTTP query sends the JSON text.
    let text = json!({"data_any": r#"{"tags": "a"}"#});
    let sets = DataSets::parse(&text).unwrap();
    assert_eq!(sets.any, Some(vec![(path("tags"), vec!["a".to_owned()])]));
    assert!(
        !DataSets::parse(&json!({"data_all": null}))
            .unwrap()
            .is_set()
    );
}

// ac:zk5119jk
#[test]
fn a_wrong_shape_is_refused() {
    let refused = |args: Value| DataSets::parse(&args).unwrap_err().to_string();
    assert_eq!(
        refused(json!({"data_any": ["a"]})),
        "data_any filter must be an object"
    );
    assert_eq!(
        refused(json!({"data_all": "tags"})),
        "data_all filter must be an object"
    );
    let path = "data_none key {key} must be a field name or a dotted path of field names";
    for key in ["", "a..b", ".a", "a b", "a/b"] {
        let args = json!({"data_none": {key: "x"}});
        assert_eq!(refused(args), path.replace("{key}", key));
    }
    let value = "data_any.tags must be a string or a list of up to 10000 strings";
    for wrong in [json!(1), json!(["a", 2]), json!({"a": "b"}), json!(null)] {
        assert_eq!(refused(json!({"data_any": {"tags": wrong}})), value);
    }
    let most: Vec<String> = (0..MOST).map(|i| i.to_string()).collect();
    assert!(DataSets::parse(&json!({"data_any": {"tags": most}})).is_ok());
    let over: Vec<String> = (0..=MOST).map(|i| i.to_string()).collect();
    assert_eq!(refused(json!({"data_any": {"tags": over}})), value);
}

// ac:zk5119jk
#[test]
fn a_value_matches_a_string_field_or_an_item_of_a_list_field() {
    let probes = probes(&path("delivery.state"), &["late".to_owned()], false);
    let want = [
        r#"{"delivery":{"state":"late"}}"#,
        r#"{"delivery":{"state":["late"]}}"#,
    ];
    assert_eq!(probes, want);
    // data_all asks for a list with every value, or for the one value as a string.
    let all = probes_of(&["a", "b"]);
    assert_eq!(all, [r#"{"tags":["a","b"]}"#]);
    let one = probes_of(&["a", "a"]);
    assert_eq!(one, [r#"{"tags":["a","a"]}"#, r#"{"tags":"a"}"#]);
}

fn probes_of(values: &[&str]) -> Vec<String> {
    let values: Vec<String> = values.iter().map(|v| (*v).to_owned()).collect();
    probes(&path("tags"), &values, true)
}

// ac:zk5119jk
#[test]
fn empty_lists_keep_as_the_rules_say() {
    let sets = DataSets::parse(&json!({"data_any": {"a": []}, "data_all": {"b": []},
        "data_none": {"c": []}}))
    .unwrap();
    let (keep, skip) = sets.groups();
    // data_any [] keeps no node: its group has no probe. data_all [] and data_none [] add none.
    assert_eq!(keep, vec![Vec::<String>::new()]);
    assert!(skip.is_empty());
    let sets = DataSets::parse(&json!({"data_all": {"b": ["x"]}, "data_none": {"c": "y"}}));
    let (keep, skip) = sets.unwrap().groups();
    assert_eq!(keep.len(), 1);
    assert_eq!(skip, vec![probes(&path("c"), &["y".to_owned()], false)]);
}

// ac:zk5119jk
#[test]
fn the_condition_reads_both_gin_indexes_and_decides_on_the_merged_data() {
    let sql = matches(4);
    assert!(sql.contains("o.computed @> ANY($4::text[]::jsonb[])"));
    assert!(sql.contains("o.document->'data' @> ANY($4::text[]::jsonb[])"));
    assert!(sql.contains(crate::computed_fields::DATA_SQL));
    // The SQL text names no field: the paths and the values are parameters.
    assert!(!sql.contains("tags"));
}
