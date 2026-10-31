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
    assert!(check_fields(&named, |_| false).is_ok());
    // A bare document field names data.<field>; a field of the tree schema is known too.
    let body = vec!["body".to_owned()];
    let data = json!({"address":"ac:a","data":{"body":"Rule"}});
    assert!(check_fields(&body, |_| false).is_ok());
    assert_eq!(
        list_item(&data, Some(&body)),
        json!({"address":"ac:a","body":"Rule"})
    );
    let custom = vec!["source_section.title".to_owned()];
    assert!(check_fields(&custom, |head| head == "source_section").is_ok());
    assert_eq!(
        check_fields(&["nope".to_owned()], |_| false)
            .unwrap_err()
            .to_string(),
        "unknown field nope"
    );
}

// read-detail-pages
#[test]
fn a_detail_gives_each_fact_once() {
    let related = json!({"target":"ac:b","relation":"related","blocking":false,"revision":null});
    let pinned = json!({"target":"task:t","relation":"depends_on","blocking":true,"revision":3});
    let stored = json!({"revision":4,"parent":"requirement:r","links":[related,pinned],
        "data":{"title":"A","note":null},"actor":"bot","actor_type":"agent","at":"2026-10-30",
        "reason":"Save","schema_revision":20,"position":8,"present":true,"contract":[4,3]});
    let reply = json!({"workspace":"w","workspace_key":"w","position":9,"address":"ac:a",
        "kind":"ac","id":"a","item_id":"w-A-1","current_revision":4,"revision_count":4,
        "contract_revision":4,"historical":false,"metadata_scope":"current",
        "document":stored,"signal":{"label":"Ready","tone":"green"},
        "review_required":false,"work_ready":true,
        "verification_summary":{"total":2,"passed":2,"failed":0},
        "links":[{"link":related,"title":"B","kind":"ac","current_revision":2,"present":true,
            "blocking":false,"revision":null,"metadata_scope":"current"},
            {"link":pinned,"title":"T","kind":"task","current_revision":5,"present":false,
            "blocking":true,"revision":3,"metadata_scope":"current"}],
        "references":[{"target":"consumer:agent","field":"consumer","title":"Agent","current_revision":2,"present":true}],
        "link_groups":[{"direction":"outgoing","relation":"related","kind":"ac","count":1},
            {"direction":"incoming","relation":"related","kind":"task","count":2}],
        "annotations":{"comments":{},"attachments":{}},
        "process":{"annotation_address":"ac:a","criterion":"ac:a","req":"requirement:r","revision":7,
            "confirmed":false,
            "assessment":{"criterion":"ac:a","req":"requirement:r","revision":7,"state":"ready","reason":"Clear"}},
        "remote_links":[],"remote_links_revision":0,"remote_review_required":false});
    // One links list: a related link has no pin, and a pin shows the current revision.
    assert_eq!(
        detail(reply.clone(), "ac:a", None).unwrap(),
        json!({"revision":4,"parent":"requirement:r","data":{"title":"A","note":null},
            "status":{"label":"Ready","tone":"green"},
            "verification_summary":{"total":2,"passed":2},
            "links":[{"target":"ac:b","title":"B"},
                {"target":"task:t","relation":"depends_on","title":"T","revision":3,
                    "current_revision":5,"blocking":true,"present":false}],
            "references":[{"target":"consumer:agent","field":"consumer","title":"Agent","current_revision":2}],
            "link_groups":[{"direction":"incoming","relation":"related","kind":"task","count":2}],
            // `req` repeats the parent; the requirement revision stays in assessment.
            "process":{"assessment":{"state":"ready","reason":"Clear","revision":7}}})
    );
    // A read by an alias names the address that the alias resolves to.
    assert_eq!(
        detail(reply.clone(), "ac:old", None).unwrap()["address"],
        "ac:a"
    );
    // An archived object, a work that is not ready and a confirmed run show.
    let mut archived = reply;
    archived["document"]["present"] = json!(false);
    archived["work_ready"] = json!(false);
    archived["process"]["confirmed"] = json!(true);
    archived["verification_summary"] = json!({"total":0,"passed":0,"failed":0});
    let archived = detail(archived, "ac:a", None).unwrap();
    assert_eq!(
        (
            &archived["present"],
            &archived["work_ready"],
            &archived["process"]["confirmed"]
        ),
        (&json!(false), &json!(false), &json!(true))
    );
    assert!(archived.get("verification_summary").is_none());
    // A revision read gives the flat document of that revision and the current revision.
    let old = json!({"revision":2,"parent":"requirement:r","links":[related],"data":{"title":"A"},
        "actor":"bot","at":"2026-10-29","reason":"Save","schema_revision":20,"position":5,
        "present":true});
    assert_eq!(
        revision(
            json!({"workspace":"w","workspace_key":"w","address":"ac:a","kind":"ac","id":"a",
            "historical":true,"current_revision":4,"contract_revision":2,"document":old,
            "position":9,"links":[{"link":related,"title":"B","current_revision":2,
            "present":true}],"remote_links":[],"remote_links_revision":0,
            "remote_review_required":false}),
            "ac:a"
        ),
        json!({"revision":2,"parent":"requirement:r","data":{"title":"A"},"current_revision":4,
            "links":[{"target":"ac:b","title":"B"}]})
    );
}
