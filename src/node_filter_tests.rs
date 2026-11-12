use super::*;
use serde_json::json;

// ac:544947gz
#[test]
fn an_empty_assignee_keeps_the_unassigned_nodes() {
    let filter = NodeFilter::parse(&json!({"assignee": ""})).unwrap();
    assert_eq!(filter.assignee.as_deref(), Some(""));
    assert!(filter.narrows());
    let query = NodeFilter::parse_query(&[("assignee", Some(""))]).unwrap();
    assert_eq!(query, filter);
    // Without the parameter, the read has no assignee filter.
    let none = NodeFilter::parse_query(&[("assignee", None)]).unwrap();
    assert_eq!(none.assignee, None);
}

// ac:q2qmqne7
#[test]
fn a_ready_filter_is_refused() {
    for args in [
        json!({"ready": true}),
        json!({"ready": false}),
        json!({"ready": "yes"}),
    ] {
        let error = NodeFilter::parse(&args).unwrap_err();
        assert_eq!(error.to_string(), "the ready filter was removed");
    }
    for value in [Some("true"), Some("")] {
        let error = NodeFilter::parse_query(&[("ready", value)]).unwrap_err();
        assert_eq!(error.to_string(), "the ready filter was removed");
    }
    assert!(NodeFilter::parse(&json!({"ready": null})).is_ok());
}
