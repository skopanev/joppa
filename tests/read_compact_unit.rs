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
    let item = json!({"address":"ac:a","title":"A","revision":3,"body":"Long",
        "signal":{"label":"Ready","tone":"green"},"present":true,"process":{"verification":"pass"}});
    assert_eq!(
        list_item(&item, None),
        json!({"address":"ac:a","title":"A","status":"green"})
    );
    let named = vec!["revision".to_owned(), "process.verification".to_owned()];
    assert_eq!(
        list_item(&item, Some(&named)),
        json!({"address":"ac:a","revision":3,"process.verification":"pass"})
    );
    // status is the tone in both shapes.
    let status = vec!["status".to_owned()];
    assert_eq!(
        list_item(&item, Some(&status)),
        json!({"address":"ac:a","status":"green"})
    );
    assert!(check_fields(&named, |_| false).is_ok());
    // A bare document field names data.<field>; a field of the tree schema is known too.
    let note = vec!["note".to_owned()];
    let data = json!({"address":"ac:a","data":{"note":"Why"}});
    assert!(check_fields(&note, |_| false).is_ok());
    assert_eq!(
        list_item(&data, Some(&note)),
        json!({"address":"ac:a","note":"Why"})
    );
    let custom = vec!["source_section.title".to_owned()];
    assert!(check_fields(&custom, |head| head == "source_section").is_ok());
    // Any other bare name reads data.<name>. A detail key that a list read does not know is
    // refused.
    assert!(check_fields(&["nope".to_owned()], |_| false).is_ok());
    assert_eq!(
        check_fields(&["comments".to_owned()], |_| false)
            .unwrap_err()
            .to_string(),
        "unknown field comments"
    );
}

// read-detail-pages
#[test]
fn a_detail_gives_each_fact_once() {
    let stored_relations = json!({"related":{"edges":[{"to":"ac:b"}]},
        "depends_on":{"edges":[{"to":"task:t","revision":3}]}});
    let stored = json!({"revision":4,"parent":"requirement:r","relations":stored_relations,
        "data":{"title":"A","note":null},"actor":"bot","at":"2026-10-30",
        "reason":"Save","schema_revision":20,"position":8,"present":true,"contract":[4,3]});
    let reply = json!({"workspace":"w","workspace_key":"w","position":9,"address":"ac:a",
        "kind":"ac","id":"a","item_id":"w-A-1","current_revision":4,"revision_count":4,
        "historical":false,"metadata_scope":"current",
        "document":stored,"signal":{"label":"Ready","tone":"green"},
        "work_ready":true,
        "relations":{"related":{"edges":[{"to":"ac:b","title":"B","current_revision":2,"present":true}]},
            "depends_on":{"edges":[{"to":"task:t","revision":3,"title":"T","current_revision":5,"present":false}]}},
        "references":[{"target":"consumer:agent","field":"consumer","title":"Agent","current_revision":2,"present":true}],
        "relation_groups":[{"direction":"incoming","relation":"related","kind":"task","count":2}],
        "annotations":{"comments":{},"attachments":{}},
        "process":{"annotation_address":"ac:a","verification":"pass"},
        "remote_links":[],"remote_links_revision":0,"remote_review_required":false});
    // The relations by key: an edge without a pin has no current revision, and a pin shows it.
    assert_eq!(
        detail(reply.clone(), "ac:a", None).unwrap(),
        json!({"revision":4,"parent":"requirement:r","data":{"title":"A","note":null},
            "status":{"label":"Ready","tone":"green"},
            "relations":{"related":{"edges":[{"to":"ac:b","title":"B"}]},
                "depends_on":{"edges":[{"to":"task:t","title":"T","revision":3,
                    "current_revision":5,"present":false}]}},
            "references":[{"target":"consumer:agent","field":"consumer","title":"Agent","current_revision":2}],
            "relation_groups":[{"direction":"incoming","relation":"related","kind":"task","count":2}],
            // The annotation address repeats the address.
            "process":{"verification":"pass"}})
    );
    // A read by an alias names the address that the alias resolves to.
    assert_eq!(
        detail(reply.clone(), "ac:old", None).unwrap()["address"],
        "ac:a"
    );
    // An archived object and a work that is not ready show.
    let mut archived = reply;
    archived["document"]["present"] = json!(false);
    archived["work_ready"] = json!(false);
    let archived = detail(archived, "ac:a", None).unwrap();
    assert_eq!(
        (&archived["present"], &archived["work_ready"]),
        (&json!(false), &json!(false))
    );
    // A revision read gives the flat document of that revision and the current revision.
    let old = json!({"revision":2,"parent":"requirement:r","relations":{"related":{"edges":[{"to":"ac:b"}]}},
        "data":{"title":"A"},"actor":"bot","at":"2026-10-29","reason":"Save","schema_revision":20,
        "position":5,"present":true});
    assert_eq!(
        revision(
            json!({"workspace":"w","workspace_key":"w","address":"ac:a","kind":"ac","id":"a",
            "historical":true,"current_revision":4,"document":old,"position":9,
            "relations":{"related":{"edges":[{"to":"ac:b","title":"B","current_revision":2,"present":true}]}},
            "remote_links":[],"remote_links_revision":0,"remote_review_required":false}),
            "ac:a"
        ),
        json!({"revision":2,"parent":"requirement:r","data":{"title":"A"},"current_revision":4,
            "relations":{"related":{"edges":[{"to":"ac:b","title":"B"}]}}})
    );
}
