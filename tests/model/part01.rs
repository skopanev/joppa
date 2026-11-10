// Part 1 of tests/model.rs, included there, so the test names stay unchanged.

#[test]
fn a_human_member_who_did_not_create_the_workspace_renames_without_changing_content() {
    // legal is a human member and did not create the workspace. agent is an agent.
    let mut w = World::new();
    let command =
        json!({"op":"update_workspace","title":"New product name","reason":"Product naming"});
    let refusal = w.apply("agent", command.clone()).unwrap_err().to_string();
    assert!(
        refusal.contains("a human member updates the workspace"),
        "{refusal}"
    );
    w.no(
        "legal",
        json!({"op":"update_workspace","title":" ","reason":"Product naming"}),
    );
    let mut expected = serde_json::to_value(&w.state).unwrap();
    expected["title"] = json!("New product name");
    w.ok("legal", command);
    assert_eq!(serde_json::to_value(&w.state).unwrap(), expected);
}
