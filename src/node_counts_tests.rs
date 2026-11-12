use super::*;
use serde_json::json;

fn parse(a: Value, sent: &[&str]) -> Result<Option<Aggregate>> {
    Aggregate::parse(&a, sent.iter().copied())
}

// ac:m9lcgtsd
#[test]
fn count_gives_the_total_and_refuses_each_page_parameter() {
    assert_eq!(
        parse(json!({"count": true}), &[]).unwrap(),
        Some(Aggregate::Count)
    );
    assert_eq!(
        parse(json!({"count": "true"}), &[]).unwrap(),
        Some(Aggregate::Count)
    );
    assert_eq!(parse(json!({"count": false}), &["limit"]).unwrap(), None);
    for name in [
        "address",
        "cursor",
        "limit",
        "fields",
        "relations_of",
        "in_relation",
        "related_to",
    ] {
        let error = parse(json!({"count": true}), &[name]).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("count cannot be combined with {name}")
        );
    }
    let error = parse(json!({"count": true, "group_by": ["kind"]}), &[]).unwrap_err();
    assert!(error.to_string().contains("group_by"), "{error}");
}

// ac:bcpvdncp
#[test]
fn group_by_takes_1_to_3_known_keys() {
    let keys = parse(json!({"group_by": ["kind", "status", "data.stage"]}), &[]).unwrap();
    let Some(Aggregate::GroupBy(keys)) = keys else {
        panic!("{keys:?}")
    };
    let sent: Vec<&str> = keys.iter().map(Key::sent).collect();
    assert_eq!(sent, ["kind", "status", "data.stage"]);
    assert_eq!(
        keys[2],
        Key::Data {
            sent: "data.stage".into(),
            field: "stage".into()
        }
    );
    let http = parse(json!({"group_by": "kind,status"}), &[]).unwrap();
    assert_eq!(http, Some(Aggregate::GroupBy(vec![Key::Kind, Key::Status])));
    for keys in [
        json!([]),
        json!(["kind", "status", "owner", "assignee"]),
        json!(""),
    ] {
        let error = parse(json!({"group_by": keys}), &[]).unwrap_err();
        assert!(
            error.to_string().starts_with("group_by takes 1 to 3 keys"),
            "{error}"
        );
    }
    for keys in [
        json!(["data."]),
        json!(["a b"]),
        json!(["kind", "kind"]),
        json!(3),
    ] {
        let error = parse(json!({"group_by": keys}), &[]).unwrap_err();
        assert!(error.to_string().contains("group_by"), "{error}");
    }
    let error = parse(json!({"group_by": ["kind"]}), &["fields"]).unwrap_err();
    assert_eq!(error.to_string(), "group_by cannot be combined with fields");
}
