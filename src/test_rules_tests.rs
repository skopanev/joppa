use super::*;
use serde_json::json;

/// Two types with the role test: `test` and `check`. `case` is a typed field of the types in `with`.
fn types(with: &[&str]) -> BTreeMap<String, TreeType> {
    let mut types = crate::objects::defaults();
    let test = types
        .values()
        .find(|t| t.role == crate::test_nodes::ROLE)
        .cloned();
    types.insert(
        "check".into(),
        test.expect("the built-in tree has a test type"),
    );
    for t in types
        .values_mut()
        .filter(|t| t.role == crate::test_nodes::ROLE)
    {
        t.fields.clear();
    }
    let rule: crate::field_types::FieldType =
        serde_json::from_value(json!({"type": "text"})).unwrap();
    for kind in with {
        types
            .get_mut(*kind)
            .unwrap()
            .fields
            .insert("case".into(), rule.clone());
    }
    types
}

fn block(field: Option<&str>) -> ResultShape {
    ResultShape {
        match_field: field.map(str::to_owned),
        ..legacy()
    }
}

#[test]
fn match_field_names_a_typed_field_of_each_test_type() {
    let both = types(&["test", "check"]);
    check_match_field(Some(&block(Some("case"))), &both).unwrap();
    for (shape, types) in [
        (block(Some("nope")), both.clone()),
        (block(Some("case")), types(&["test"])),
        (block(Some("case")), types(&[])),
    ] {
        let error = check_match_field(Some(&shape), &types).unwrap_err();
        assert_eq!(error.to_string(), MATCH_FIELD);
    }
    // Without a type of the role test, no type has the field.
    let mut none = both.clone();
    none.retain(|_, t| t.role != crate::test_nodes::ROLE);
    let error = check_match_field(Some(&block(Some("case"))), &none).unwrap_err();
    assert_eq!(error.to_string(), MATCH_FIELD);
    // A block without match_field is never refused for it.
    check_match_field(Some(&block(None)), &none).unwrap();
    check_match_field(None, &none).unwrap();
}

#[test]
fn match_field_is_optional_and_a_number_is_refused() {
    let mut raw = serde_json::to_value(legacy()).unwrap();
    assert!(raw.get("match_field").is_none());
    raw["match_field"] = json!("case");
    let shape: ResultShape = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(shape.match_field.as_deref(), Some("case"));
    assert_eq!(serde_json::to_value(&shape).unwrap()["match_field"], "case");
    raw["match_field"] = json!(7);
    let error = parse_block(&crate::model::Workspace::default(), &raw).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid test_results: invalid type: integer `7`, expected a string"
    );
}
