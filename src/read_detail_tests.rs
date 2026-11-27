use super::*;

/// A full object read (store::read_objects) of `ac:a` with one active and one withdrawn
/// comment, one file, one reference, one incoming group and child counts.
fn reply() -> Value {
    json!({"workspace": "w", "position": 3, "address": "ac:a", "kind": "ac", "id": "a",
        "current_revision": 2,
        "document": {"revision": 2, "parent": "requirement:r", "data": {"title": "A"}},
        "signal": {"label": "Current document", "tone": "blue"},
        "process": {"annotation_address": "ac:a"},
        "annotations": {"comments": {
            "c1": {"actor": "bot", "at": "2026-01-01", "text": "x", "withdrawn": false,
                "author": "bot", "created_at": "2026-01-01"},
            "c2": {"actor": "bot", "at": "2026-01-02", "text": "y", "withdrawn": true}}},
        "files": [{"id": "f", "name": "a.txt", "size": 1, "hash": "h", "url": "https://x",
            "at": "t", "content_type": "text/plain", "actor": "bot", "created_at": "t"}],
        "references": [{"target": "consumer:agent", "field": "consumer", "title": "Agent",
            "current_revision": 1, "present": true}],
        "relation_groups": [{"direction": "incoming", "relation": "related", "kind": "note",
            "count": 1}],
        "child_counts": [{"kind": "test", "total": 2, "passed": 1}]})
}

#[test]
fn a_card_gives_the_counts_and_no_fact_that_comes_on_request() {
    let card = detail(reply(), "ac:a", None).unwrap();
    assert_eq!(
        card,
        json!({"revision": 2, "parent": "requirement:r", "data": {"title": "A"},
            "status": {"label": "Current document", "tone": "blue"}, "comments_count": 1,
            "files_count": 1,
            "relation_groups": [{"relation": "related", "kind": "note", "count": 1}]})
    );
}

#[test]
fn the_detail_keys_give_the_comment_page_the_short_files_and_the_references() {
    let keys = detail_keys(reply(), "ac:a", None).unwrap();
    assert_eq!(
        keys["comments"],
        json!({"items": [{"id": "c1", "actor": "bot", "at": "2026-01-01", "text": "x"},
            {"id": "c2", "actor": "bot", "at": "2026-01-02", "text": "y", "withdrawn": true}],
            "total": 2})
    );
    assert_eq!(
        keys["files"],
        json!([{"id": "f", "name": "a.txt", "size": 1, "content_type": "text/plain", "at": "t"}])
    );
    assert_eq!(
        keys["references"],
        json!([{"target": "consumer:agent", "field": "consumer", "title": "Agent",
            "current_revision": 1}])
    );
    assert_eq!(keys["child_counts"], reply()["child_counts"]);
}

#[test]
fn a_count_of_0_shows_only_on_request() {
    let mut empty = reply();
    empty["annotations"] = json!({});
    empty["files"] = json!([]);
    let keys = detail_keys(empty.clone(), "ac:a", None).unwrap();
    assert_eq!(keys["comments_count"], json!(0));
    assert_eq!(keys["files_count"], json!(0));
    let card = detail(empty, "ac:a", None).unwrap();
    assert!(card.get("comments_count").is_none() && card.get("files_count").is_none());
}

#[test]
fn the_limit_of_a_read_of_one_address_sizes_the_comment_page() {
    let mut reply = reply();
    for n in 3..=5 {
        reply["annotations"]["comments"][format!("c{n}")] =
            json!({"actor": "bot", "at": format!("2026-01-0{n}"), "text": "z"});
    }
    let first = detail_keys_sized(reply.clone(), "ac:a", (None, 2)).unwrap();
    assert_eq!(first["comments"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        (
            &first["comments"]["total"],
            &first["comments"]["next_cursor"]
        ),
        (&json!(5), &json!("2"))
    );
    assert_eq!(
        first["comments_active"]["items"].as_array().unwrap().len(),
        2
    );
    assert_eq!(first["comments_active"]["total"], json!(4));
    let last = detail_keys_sized(reply, "ac:a", (Some("4"), 2)).unwrap();
    assert_eq!(last["comments"]["items"].as_array().unwrap().len(), 1);
    assert!(
        last["comments"].get("next_cursor").is_none(),
        "{}",
        last["comments"]
    );
}
