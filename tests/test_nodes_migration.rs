//! migrate_test_nodes turns each test list into test nodes: a type with role test and status
//! own under each criterion type, one node for each listed test, and no list in the document.
mod common;
use common::*;
use serde_json::json;

/// Apply an internal migration event, as the journal replay does.
fn internal(w: &mut World, op: &str) -> anyhow::Result<()> {
    let command: joppa::commands::Command = serde_json::from_value(json!({ "op": op }))?;
    let mut copy = w.state.clone();
    joppa::commands::apply_replay(
        &mut copy,
        "owner",
        &command,
        w.position + 1,
        "2026-01-01T00:00:00Z",
    )?;
    w.state = copy;
    w.position += 1;
    Ok(())
}

// test-nodes-ac1
#[test]
fn the_migration_turns_each_test_list_into_test_nodes() {
    let mut w = World::new();
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_test_rules",
    ] {
        internal(&mut w, op).unwrap();
    }
    // A test list of an old journal.
    let names = ["tests/a.rs > one", "tests/a.rs > two"];
    w.legacy(
        "owner",
        json!({"op":"set_ac_tests","target":"ac:ios","tests":names,"reason":"Bind"}),
    );
    let before = w.state.objects["ac:ios"].clone();
    internal(&mut w, "migrate_test_nodes").unwrap();
    let schema = joppa::objects::schema(&w.state);
    let test = &schema.types["test"];
    assert_eq!(
        (test.role.as_str(), test.status.as_deref()),
        ("test", Some("own"))
    );
    assert!(
        test.parents.contains(&"ac".to_owned()),
        "{:?}",
        test.parents
    );
    assert_eq!(schema.types["ac"].status.as_deref(), Some("children"));
    // One node for each name, under the criterion, with the owner of the criterion.
    let mut listed = joppa::test_nodes::names_by_parent(&w.state)
        .remove("ac:ios")
        .unwrap_or_default();
    listed.sort();
    assert_eq!(listed, names);
    for name in names {
        let id = joppa::test_nodes::node_id("ac:ios", name);
        let node = w.state.objects[&format!("test:{id}")].current();
        assert_eq!(node.parent.as_deref(), Some("ac:ios"));
        assert_eq!(node.data["owner"], before.current().data["owner"]);
    }
    // The list leaves the document; the contract revision stays, so pins and reviews stay.
    let after = &w.state.objects["ac:ios"];
    assert!(after.current().data.get("tests").is_none());
    assert_eq!(
        joppa::objects::contract_revision(after),
        joppa::objects::contract_revision(&before)
    );
    // The event runs once.
    let count = w.state.objects.len();
    internal(&mut w, "migrate_test_nodes").unwrap();
    assert_eq!(w.state.objects.len(), count);
    // After it a list is refused, a node title must be a test name, and a parent names each
    // test once.
    let twice = joppa::test_nodes::check_save(
        &w.state,
        "test",
        "test:new",
        Some("ac:ios"),
        &json!({"title":names[0]}),
    );
    assert_eq!(
        twice.unwrap_err().to_string(),
        format!("test \"{}\" is listed twice", names[0])
    );
    let refused = joppa::test_nodes::check_save(
        &w.state,
        "ac",
        "ac:ios",
        Some("requirement:r"),
        &json!({"title":"A","tests":["x"]}),
    );
    assert_eq!(refused.unwrap_err().to_string(), joppa::test_nodes::REFUSAL);
    assert!(
        joppa::test_nodes::check_save(
            &w.state,
            "test",
            "test:new",
            Some("ac:ios"),
            &json!({"title":"no file"})
        )
        .is_err()
    );
    assert!(
        joppa::test_nodes::check_save(
            &w.state,
            "test",
            "test:new",
            Some("ac:ios"),
            &json!({"title":"tests/a.rs > three"})
        )
        .is_ok()
    );
}
