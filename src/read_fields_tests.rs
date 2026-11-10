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
