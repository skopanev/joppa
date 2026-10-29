//! requirement:read-contract, the pure shape rules without a server.
use joppa::read_compact::{
    check_fields, detail, list_item, page, revision, strip_empty, strip_reply,
};
use serde_json::json;

// read-empty-verbose-tests
#[test]
fn empty_keys_go_outside_authored_data_and_false_and_0_stay() {
    let mut reply = json!({"a":null,"b":"","c":[],"d":{},"e":false,"f":0,
        "g":{"h":null,"i":1},"document":{"data":{"note":null,"projects":[]}},"data":{"x":""},
        "command":{"y":null},"remote_links":[{"target":"ac:x","revision":null,"status":{}}]});
    strip_empty(&mut reply);
    assert_eq!(
        reply,
        json!({"e":false,"f":0,"g":{"i":1},"document":{"data":{"note":null,"projects":[]}},
            "data":{"x":""},"command":{"y":null},
            "remote_links":[{"target":"ac:x","revision":null,"status":{}}]})
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
    // status is the tone in both shapes.
    let status = vec!["status".to_owned()];
    assert_eq!(
        list_item(&item, Some(&status)),
        json!({"address":"ac:a","status":"green"})
    );
    assert!(check_fields(&named).is_ok());
    assert_eq!(
        check_fields(&["nope".to_owned()]).unwrap_err().to_string(),
        "unknown field nope"
    );
}

// read-detail-pages
#[test]
fn a_detail_gives_each_fact_once() {
    let stored = json!({"revision":4,"parent":"requirement:r","links":[{"target":"ac:b","relation":"related","blocking":false,"revision":null}],"data":{"title":"A"}});
    let reply = json!({"workspace":"w","workspace_key":"w","position":9,"address":"ac:a",
        "kind":"ac","id":"a","item_id":"w-A-1","current_revision":4,"revision_count":4,
        "contract_revision":4,"historical":false,"metadata_scope":"current",
        "document":stored,"signal":{"label":"Ready","tone":"green"},
        "review_required":false,"work_ready":true,
        "links":[{"link":{"target":"ac:b","relation":"related","blocking":false,"revision":null},
            "title":"B","kind":"ac","current_revision":2,"present":true,"blocking":false,
            "revision":null,"metadata_scope":"current"}],
        "references":[{"target":"consumer:agent","field":"consumer","title":"Agent","current_revision":2,"present":true}],
        "link_groups":[{"direction":"outgoing","relation":"related","kind":"ac","count":1},
            {"direction":"incoming","relation":"related","kind":"task","count":2}],
        "annotations":{"comments":{},"attachments":{}},
        "process":{"annotation_address":"ac:a","criterion":"ac:a","req":"requirement:r","revision":7,
            "assessment":{"criterion":"ac:a","req":"requirement:r","revision":7,"state":"ready","reason":"Clear"}},
        "remote_links":[],"remote_links_revision":0,"remote_review_required":false});
    assert_eq!(
        detail(reply, None, None).unwrap(),
        json!({"workspace":"w","position":9,"address":"ac:a","current_revision":4,"document":stored,
            "status":{"label":"Ready","tone":"green"},"review_required":false,"work_ready":true,
            "links":{"items":[{"target":"ac:b","relation":"related","title":"B","current_revision":2}],"total":1},
            "references":[{"target":"consumer:agent","field":"consumer","title":"Agent","current_revision":2}],
            "link_groups":[{"direction":"incoming","relation":"related","kind":"task","count":2}],
            // `req` repeats document.parent, so the process has no `req`.
            "process":{"revision":7,"assessment":{"state":"ready","reason":"Clear"}}})
    );
    // A revision read follows the same rules for copies and an empty remote link list.
    assert_eq!(
        revision(
            json!({"workspace":"w","workspace_key":"w","address":"ac:a","kind":"ac","id":"a",
            "historical":true,"current_revision":4,"remote_links":[],"remote_links_revision":0,
            "remote_review_required":false})
        ),
        json!({"workspace":"w","address":"ac:a","historical":true,"current_revision":4})
    );
}
