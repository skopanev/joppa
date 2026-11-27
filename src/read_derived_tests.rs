use super::*;

#[test]
fn an_item_gives_its_excerpt_data_keys_and_status_label() {
    let body = format!("A  first\n\nline {}", "x".repeat(300));
    let item = json!({"body": body, "data": {"title": "T", "stage": "draft", "gone": null},
        "computed": {"sources": ["source:s"]}, "signal": {"label": "2/2 passed", "tone": "green"}});
    let excerpt = derived(&item, "excerpt").unwrap();
    let excerpt = excerpt.as_str().unwrap();
    assert!(excerpt.starts_with("A first line x"), "{excerpt}");
    assert_eq!(excerpt.chars().count(), 200);
    assert_eq!(
        derived(&item, "data_keys").unwrap(),
        json!(["sources", "stage", "title"])
    );
    assert_eq!(derived(&item, "status_label").unwrap(), json!("2/2 passed"));
    let empty = json!({"body": " ", "signal": {"label": ""}});
    assert_eq!(derived(&empty, "excerpt").unwrap(), Value::Null);
    assert_eq!(derived(&empty, "status_label").unwrap(), Value::Null);
    assert_eq!(derived(&empty, "data_keys").unwrap(), json!([]));
    assert_eq!(derived(&item, "title"), None);
}

#[test]
fn an_unknown_address_of_an_unknown_type_names_the_valid_types() {
    let types = ["requirement".to_owned(), "ac".to_owned()];
    assert_eq!(
        unknown_object("req:x", &types),
        "unknown object: req:x (unknown type req, valid types are ac, requirement)"
    );
    assert_eq!(unknown_object("ac:x", &types), "unknown object: ac:x");
    assert_eq!(
        unknown_object("joppa-R-1", &types),
        "unknown object: joppa-R-1"
    );
}
