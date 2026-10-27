//! migrate_test_rules writes every test rule and the status rule of each type into the
//! tree schema. After it, a schema change must send the whole block.
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

/// A workspace after the earlier migrations and before migrate_test_rules.
fn before() -> World {
    let mut w = World::new();
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
    ] {
        internal(&mut w, op).unwrap();
    }
    assert!(!w.state.test_rules_migrated);
    w
}

// test-rules-in-schema-ac1
#[test]
fn the_migration_writes_the_older_rules_and_the_rule_of_each_type() {
    let mut w = before();
    let old = joppa::objects::schema(&w.state);
    let fields: Vec<_> = old
        .types
        .keys()
        .map(|kind| joppa::objects::process_fields(&w.state, kind))
        .collect();
    internal(&mut w, "migrate_test_rules").unwrap();
    let new = joppa::objects::schema(&w.state);
    assert_eq!(new.revision, old.revision + 1);
    assert_eq!(new.reason, joppa::test_rules::REASON);
    for ((kind, t), fields) in new.types.iter().zip(fields) {
        let rule = joppa::objects::role_status_rule(&old.types[kind]);
        assert_eq!(t.status.as_deref(), Some(rule), "{kind}");
        assert_eq!(t.process_fields.as_ref(), Some(&fields), "{kind}");
    }
    assert_eq!(new.test_results, Some(joppa::test_rules::legacy()));
    // The event runs once.
    internal(&mut w, "migrate_test_rules").unwrap();
    assert_eq!(joppa::objects::schema(&w.state).revision, new.revision);
}

// test-rules-in-schema-ac1
#[test]
fn the_migration_keeps_the_status_names_and_fields_of_an_older_block() {
    let mut w = before();
    let s = joppa::objects::schema(&w.state);
    w.ok("owner", json!({"op":"set_tree_schema","revision":s.revision,"reason":"An older block",
        "test_results":{"statuses":{"passed":"pass","broken":"fail"},"fields":[{"name":"app","type":"text"}]}}));
    internal(&mut w, "migrate_test_rules").unwrap();
    let block = joppa::objects::schema(&w.state).test_results.unwrap();
    assert_eq!(
        serde_json::to_value(&block.statuses).unwrap(),
        json!({"passed":"pass","broken":"fail"})
    );
    let names: Vec<_> = block.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["message", "duration_ms", "app"]);
    assert_eq!(block.fields[2].max_length, Some(1024));
    joppa::result_shape::validate(&block).unwrap();
}

// test-rules-in-schema-ac2
#[test]
fn after_the_migration_a_schema_change_sends_every_rule() {
    let mut w = before();
    internal(&mut w, "migrate_test_rules").unwrap();
    let s = joppa::objects::schema(&w.state);
    let partial = json!({"op":"set_tree_schema","revision":s.revision,"reason":"Part of the rules",
        "test_results":{"statuses":{"ok":"pass","bad":"fail"}}});
    let error = w.apply("owner", partial).unwrap_err().to_string();
    assert!(
        error.contains("test_results needs every rule; missing: fields, name"),
        "{error}"
    );
    let mut block = serde_json::to_value(joppa::test_rules::legacy()).unwrap();
    block["max_tests"] = json!(3);
    w.ok("owner", json!({"op":"set_tree_schema","revision":s.revision,"reason":"Fewer tests","test_results":block}));
    assert_eq!(
        joppa::objects::schema(&w.state)
            .test_results
            .unwrap()
            .max_tests,
        3
    );
    // With status_required, a type without a status rule is refused.
    let s = joppa::objects::schema(&w.state);
    assert_eq!(s.status_required, Some(true));
    let mut types = s.types.clone();
    types.get_mut("ac").unwrap().status = None;
    let no_rule =
        json!({"op":"set_tree_schema","revision":s.revision,"reason":"No rule","types":types});
    let error = w.apply("owner", no_rule.clone()).unwrap_err().to_string();
    assert!(error.contains("type ac needs a status rule"), "{error}");
    // Without the flag the schema saves the type, and the type has the rule unset.
    let mut without = no_rule;
    without["status_required"] = json!(false);
    w.ok("owner", without);
    let s = joppa::objects::schema(&w.state);
    assert_eq!(s.status_required, Some(false));
    assert_eq!(joppa::objects::status_rule(&s.types["ac"]), "unset");
}
