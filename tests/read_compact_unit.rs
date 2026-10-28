//! requirement:read-contract, the pure shape rules without a server.
use joppa::read_compact::{check_fields, list_item, page, strip_empty, strip_reply};
use serde_json::json;

// read-empty-verbose-tests
#[test]
fn empty_keys_go_outside_authored_data_and_false_and_0_stay() {
    let mut reply = json!({"a":null,"b":"","c":[],"d":{},"e":false,"f":0,
        "g":{"h":null,"i":1},"document":{"data":{"note":null,"projects":[]}},"data":{"x":""},
        "command":{"y":null}});
    strip_empty(&mut reply);
    assert_eq!(
        reply,
        json!({"e":false,"f":0,"g":{"i":1},"document":{"data":{"note":null,"projects":[]}},
            "data":{"x":""},"command":{"y":null}})
    );
    assert_eq!(
        strip_reply(json!({"items":[],"next_cursor":null})),
        json!({"items":[]})
    );
}

// read-detail-pages
#[test]
fn a_page_gives_25_items_a_cursor_and_the_total() {
    let entries: Vec<_> = (0..30).map(|i| json!(i)).collect();
    let first = page(entries.clone(), None).unwrap();
    assert_eq!(first["items"].as_array().unwrap().len(), 25);
    assert_eq!(first["next_cursor"], "25");
    assert_eq!(first["total"], 30);
    let last = page(entries.clone(), Some("25")).unwrap();
    assert_eq!(last["items"], json!([25, 26, 27, 28, 29]));
    assert!(last.get("next_cursor").is_none());
    assert!(page(entries, Some("x")).is_err());
}

// read-list-fields
#[test]
fn a_list_item_is_short_and_fields_keep_each_named_path() {
    let item = json!({"address":"ac:a","title":"A","revision":3,"description":"Long",
        "signal":{"label":"Ready","tone":"green"},"present":true,"process":{"planning_claim":null}});
    assert_eq!(
        list_item(&item, None),
        json!({"address":"ac:a","title":"A","status":"green"})
    );
    let named = vec!["revision".to_owned(), "process.planning_claim".to_owned()];
    assert_eq!(
        list_item(&item, Some(&named)),
        json!({"address":"ac:a","revision":3,"process.planning_claim":null})
    );
    assert!(check_fields(&named).is_ok());
    assert_eq!(
        check_fields(&["nope".to_owned()]).unwrap_err().to_string(),
        "unknown field nope"
    );
}
