use super::*;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_bare_name_reads_its_data_field_and_an_absent_one_gives_null() {
    check_fields(&names(&["tags", "nope", "data.tags", "relations"]), |_| {
        false
    })
    .unwrap();
    let item = json!({"address": "ac:a", "data": {"tags": ["kyc"]}});
    assert_eq!(value(&item, None, "tags"), json!(["kyc"]));
    assert_eq!(value(&item, None, "data.tags"), json!(["kyc"]));
    assert_eq!(value(&item, None, "nope"), Value::Null);
}

#[test]
fn an_address_read_gives_child_counts_and_the_system_comment_count() {
    let counts = json!([{"kind": "test", "total": 2, "passed": 1}]);
    let reply = json!({"address": "ac:a", "document": {"data": {"comments_count": 7}},
        "child_counts": counts});
    let detail = json!({"comments_count": 0});
    let fields = names(&["child_counts", "comments_count"]);
    assert_eq!(
        object_fields(&reply, Some(&detail), &fields),
        json!({"address": "ac:a", "child_counts": counts, "comments_count": 0})
    );
}

#[test]
fn narrow_fields_need_only_the_object_row() {
    let needs = card_needs(Some(&names(&["title", "status", "status.label", "body"])));
    assert_eq!(needs, CardNeeds::default());
    let needs = card_needs(Some(&names(&["comments_count", "tags"])));
    assert!(needs.counts && needs.document && !needs.notes && !needs.edges && !needs.groups);
    // The card counts the comments and the files, and signs and lists nothing.
    let card = card_needs(None);
    assert_eq!(card, CardNeeds::CARD);
    assert!(card.counts && !card.notes && !card.references);
}

#[test]
fn a_detail_key_on_a_list_and_an_older_text_field_stay_refused() {
    let error = check_fields(&names(&["comments"]), |_| false).unwrap_err();
    assert_eq!(error.to_string(), "unknown field comments");
    // A read by address takes the detail keys.
    check_fields(&names(&["comments"]), |head| head == "comments").unwrap();
    for field in ["description", "data.description"] {
        let error = check_fields(&names(&[field]), |_| true).unwrap_err();
        assert_eq!(
            error.to_string(),
            "use body instead of description; read joppa_help"
        );
    }
}
