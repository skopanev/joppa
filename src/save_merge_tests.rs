use super::*;
use serde_json::json;

#[test]
fn only_an_update_without_data_and_without_clean_gets_empty_data() {
    let mut update = json!({"op":"save_object","kind":"ac","id":"b","revision":2});
    default_data(&mut update, false);
    assert_eq!(update["data"], json!({}));
    // A create and a clean save still need data, and sent data stays as sent.
    for (mut value, clean) in [
        (
            json!({"op":"save_object","kind":"ac","id":"c","revision":0}),
            false,
        ),
        (
            json!({"op":"save_object","kind":"ac","id":"b","revision":4}),
            true,
        ),
        (
            json!({"op":"archive_object","target":"ac:b","revision":4}),
            false,
        ),
    ] {
        default_data(&mut value, clean);
        assert!(value.get("data").is_none(), "{value}");
    }
    let mut sent = json!({"op":"save_object","kind":"ac","id":"b","revision":2,"data":{"x":1}});
    default_data(&mut sent, false);
    assert_eq!(sent["data"], json!({"x":1}));
}

#[test]
fn a_live_save_refuses_relations_and_links_inside_data_but_accepts_null() {
    let w = Workspace::default();
    let refusal = |field: &str| {
        format!(
            "data.{field} is not a data field of ac; send relations at the top level of save_object"
        )
    };
    let sent = json!({"relations": {"related": {"edges": []}}});
    let error = relations_in_data(&w, "ac", &sent).unwrap_err();
    assert_eq!(error.to_string(), refusal("relations"));
    let error = relations_in_data(&w, "ac", &json!({"links": []})).unwrap_err();
    assert_eq!(error.to_string(), refusal("links"));
    relations_in_data(&w, "ac", &json!({"links": null, "relations": null})).unwrap();
    relations_in_data(&w, "ac", &json!({"title": "T"})).unwrap();
}
