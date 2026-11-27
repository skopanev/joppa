//! The order clauses, their SQL and the cursor of a list with order (dependency_order.rs).
use super::*;
use serde_json::json;

fn parse(value: Value) -> Result<Option<Order>> {
    Order::parse(&value)
}

fn types() -> BTreeMap<String, TreeType> {
    let schema = json!({"step": {"title": "Step", "role": "task",
        "fields": {"stage": {"type": "enum", "values": ["todo", "doing", "done"]},
            "size": {"type": "number"}}}});
    serde_json::from_value(schema).unwrap()
}

#[test]
fn parse_takes_text_and_lists() {
    let order = parse(json!("dependents desc, data.priority asc,age"))
        .unwrap()
        .unwrap();
    let keys = vec![
        (Key::Dependents, true),
        (Key::Priority, false),
        (Key::Age, false),
    ];
    assert_eq!(order, Order(keys));
    let order = parse(json!(["created desc", "data.stage", "size desc"]))
        .unwrap()
        .unwrap();
    assert_eq!(order.0[0], (Key::Age, true));
    assert_eq!(order.0[1], (Key::Field("stage".into()), false));
    assert_eq!(order.0[2], (Key::Field("size".into()), true));
    assert_eq!(parse(Value::Null).unwrap(), None);
    // The address order alone is the default order.
    assert_eq!(parse(json!("address")).unwrap(), None);
    assert_eq!(
        parse(json!("address desc")).unwrap().unwrap().0[0],
        (Key::Address, true)
    );
}

#[test]
fn parse_refuses_bad_clauses() {
    let refusal = |value: Value| parse(value).unwrap_err().to_string();
    assert_eq!(refusal(json!([])), "order takes 1 to 4 clauses");
    assert_eq!(refusal(json!("a,b,c,d,e")), "order takes 1 to 4 clauses");
    assert_eq!(refusal(json!("")), KEYS);
    assert_eq!(refusal(json!("bad'key")), KEYS);
    assert_eq!(
        refusal(json!("age up")),
        "order direction must be asc or desc"
    );
    assert_eq!(
        refusal(json!("age asc desc")),
        "order direction must be asc or desc"
    );
    assert_eq!(
        refusal(json!("priority,data.priority")),
        "order names each key once"
    );
    assert_eq!(
        refusal(json!(7)),
        "order must be a string or a list of strings"
    );
    assert_eq!(
        refusal(json!([7])),
        "order must be a string or a list of strings"
    );
}

#[test]
fn sql_binds_names_and_ranks() {
    let types = types();
    let priorities = ["p0".to_owned(), "p1".to_owned()];
    let order = parse(json!("dependents desc,priority,stage,size desc"))
        .unwrap()
        .unwrap();
    let inputs = Inputs {
        facts: Some("{}".into()),
        priorities: &priorities,
        types: &types,
    };
    let (sql, params) = order.sql(19, inputs);
    // The facts, the priorities, the name priority, the name stage, its values, the name size.
    assert_eq!(params.len(), 6);
    assert!(sql.starts_with("COALESCE((COALESCE($19::text,'{}')::jsonb->o.address->>'dependents')::bigint,0) DESC NULLS LAST,"));
    assert!(sql.contains("array_position($20::text[],NULLIF((COALESCE(o.document->'data'->$21::text,o.computed->$21::text))#>>'{}','')) ASC NULLS LAST"));
    assert!(
        sql.contains("array_position($23::text[],NULLIF((COALESCE(o.document->'data'->$22::text")
    );
    assert!(sql.contains(
        "jsonb_typeof(COALESCE(o.document->'data'->$24::text,o.computed->$24::text))='number'"
    ));
    assert!(sql.ends_with(",o.address ASC"));
    assert!(!sql.contains("stage") && !sql.contains("size"));
}

#[test]
fn sql_of_columns_needs_no_parameter() {
    let order = parse(json!("age desc,title,revision desc,address desc"))
        .unwrap()
        .unwrap();
    let inputs = Inputs {
        facts: None,
        priorities: &[],
        types: &BTreeMap::new(),
    };
    let (sql, params) = order.sql(1, inputs);
    assert!(params.is_empty());
    assert_eq!(
        sql,
        "COALESCE(o.created_position,0) DESC NULLS LAST,o.title ASC NULLS LAST,o.revision DESC NULLS LAST,o.address DESC NULLS LAST"
    );
    assert!(!order.needs_facts() && !order.needs_priorities());
}

#[test]
fn conflicting_enum_values_sort_as_values() {
    let mut types = types();
    let mut other = types["step"].clone();
    other.fields.get_mut("stage").unwrap().values.reverse();
    types.insert("other".into(), other);
    assert_eq!(enum_values(&types, "stage"), None);
    assert_eq!(enum_values(&types, "size"), None);
    assert_eq!(enum_values(&self::types(), "stage").unwrap()[0], "todo");
}

#[test]
fn order_applies_only_to_lists() {
    assert!(from_args(&json!({"order": "age"})).unwrap().is_some());
    assert!(from_args(&json!({"order": "age", "count": false})).is_ok());
    for key in ["address", "relations_of", "in_relation", "group_by"] {
        let refusal = from_args(&json!({"order": "age", key: "x"})).unwrap_err();
        assert_eq!(
            refusal.to_string(),
            "order applies only to object list reads"
        );
    }
    assert!(from_args(&json!({"order": "age", "count": true})).is_err());
    assert!(from_args(&json!({"address": "ac:a"})).unwrap().is_none());
}

#[test]
fn cursor_keeps_the_snapshot() {
    assert_eq!(offset(None, 9).unwrap(), 0);
    assert_eq!(offset(Some("9:@25"), 9).unwrap(), 25);
    assert_eq!(next_cursor(9, 25, 25), "9:@50");
    let changed = offset(Some("8:@25"), 9).unwrap_err().to_string();
    assert_eq!(changed, "object list changed, start again without cursor");
    for bad in ["9:@0", "9:@x", "9:ac:a", "ac:a"] {
        assert_eq!(offset(Some(bad), 9).unwrap_err().to_string(), CURSOR);
    }
}
