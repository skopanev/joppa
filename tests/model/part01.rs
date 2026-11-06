// Part 1 of tests/model.rs, included there, so the test names stay unchanged.

#[test]
fn only_workspace_owner_can_rename_without_changing_content() {
    let mut w = World::new();
    let command =
        json!({"op":"update_workspace","title":"New product name","reason":"Product naming"});
    w.no("agent", command.clone());
    w.no("legal", command.clone());
    w.no(
        "owner",
        json!({"op":"update_workspace","title":" ","reason":"Product naming"}),
    );
    let mut expected = serde_json::to_value(&w.state).unwrap();
    expected["title"] = json!("New product name");
    w.ok("owner", command);
    assert_eq!(serde_json::to_value(&w.state).unwrap(), expected);
}

