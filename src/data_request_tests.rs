use super::*;
use serde_json::json;

/// A schema with a root type and two child types. Each name is a test name.
fn types() -> BTreeMap<String, TreeType> {
    serde_json::from_value(json!({
        "box": {"title": "Box", "role": "group", "root": true},
        "type_a": {"title": "A", "role": "document", "parents": ["box"], "fields": {
            "area": {"type": "text"},
            "retries": {"type": "number"},
            "strict": {"type": "boolean"},
            "env": {"type": "enum", "values": ["dev", "prod"]},
            "tags": {"type": "list"}
        }},
        "type_b": {"title": "B", "role": "document", "parents": ["type_a"], "fields": {
            "lane": {"type": "enum", "values": ["fast", "slow"], "from": "parent"}
        }}
    }))
    .unwrap()
}

fn plan(body: Value) -> Result<Plan> {
    serde_json::from_value::<Request>(body)?.plan(&types())
}

fn status(body: Value) -> u16 {
    let e = plan(body).unwrap_err();
    e.downcast_ref::<Refusal>().map_or(0, |r| r.status)
}

#[test]
fn an_empty_request_walks_every_type_with_title_and_body() {
    let p = plan(json!({})).unwrap();
    assert_eq!(p.kinds, None);
    assert_eq!(p.fields, ["title", "body"]);
    assert_eq!((p.limit, p.max_chars), (None, MAX_CHARS));
}

#[test]
fn a_filter_takes_each_declared_field_type_and_owner_and_assignee() {
    let p = plan(json!({"filter": {
        "area": "x", "retries": [1, 3], "strict": true, "env": ["dev", "prod"],
        "tags": "db", "lane": "fast", "owner": "m1", "assignee": ["a1"]
    }}))
    .unwrap();
    assert_eq!(p.filter.len(), 8);
    let retries = p.filter.iter().find(|(f, _)| f == "retries").unwrap();
    assert_eq!(retries.1, vec![json!(1), json!(3)]);
}

#[test]
fn a_filter_refuses_undeclared_fields_wrong_types_and_empty_lists() {
    for filter in [
        json!({"nope": 1}),
        json!({"retries": "3"}),
        json!({"retries": [1, "2"]}),
        json!({"strict": "true"}),
        json!({"env": 1}),
        json!({"tags": []}),
        json!({"area": null}),
        json!({"owner": 5}),
    ] {
        assert_eq!(status(json!({ "filter": filter })), 400, "{filter}");
    }
}

#[test]
fn kind_takes_a_name_or_a_list_of_schema_types() {
    let p = plan(json!({"kind": "type_b"})).unwrap();
    assert_eq!(p.kinds, Some(["type_b".to_owned()].into()));
    let p = plan(json!({"kind": ["type_a", "type_b"]})).unwrap();
    assert_eq!(p.kinds.unwrap().len(), 2);
    for kind in [json!("nope"), json!([]), json!(["type_a", 1]), json!(3)] {
        assert_eq!(status(json!({ "kind": kind })), 400, "{kind}");
    }
}

#[test]
fn fields_take_the_fixed_keys_and_declared_fields_only() {
    let p = plan(json!({"fields": ["area", "status", "parent", "owner", "assignee"]})).unwrap();
    assert_eq!(p.fields.len(), 5);
    for fields in [json!(["work_ready"]), json!(["nope"]), json!([])] {
        assert_eq!(status(json!({ "fields": fields })), 400, "{fields}");
    }
}

#[test]
fn limits_depend_on_the_query() {
    assert_eq!(plan(json!({"query": "word"})).unwrap().limit, Some(10));
    assert_eq!(
        plan(json!({"query": "w", "limit": 25})).unwrap().limit,
        Some(25)
    );
    assert_eq!(plan(json!({"limit": 1000})).unwrap().limit, Some(1000));
    let long = "x".repeat(2001);
    for body in [
        json!({"query": "   "}),
        json!({"query": long}),
        json!({"query": "w", "limit": 0}),
        json!({"query": "w", "limit": 26}),
        json!({"limit": 0}),
        json!({"limit": 1001}),
        json!({"max_chars": 0}),
        json!({"max_chars": 262145}),
    ] {
        assert_eq!(status(body.clone()), 400, "{body}");
    }
    assert_eq!(
        plan(json!({"max_chars": 262144})).unwrap().max_chars,
        262144
    );
}

#[test]
fn an_unknown_body_key_is_refused() {
    assert!(serde_json::from_value::<Request>(json!({"extra": 1})).is_err());
}

#[test]
fn a_read_key_is_the_prefix_and_64_lowercase_hex_characters() {
    let hex = "0123456789abcdef".repeat(4);
    assert!(is_read_key(&format!("jpr_{hex}")));
    assert!(!is_read_key(&format!("jpr_{}", hex.to_uppercase())));
    assert!(!is_read_key(&format!("jpr_{hex}0")));
    assert!(!is_read_key(&format!("jpa_{hex}")));
    assert!(!is_read_key("abc"));
}
