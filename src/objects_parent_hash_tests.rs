//! The parent hash of a node: the stamp of a save, the quiet saves, the confirmation and the
//! refusals (parent_hash.rs).
use super::*;
use crate::commands::{Command, apply, apply_inner, unchanged_save};

pub(crate) const AT: &str = "2026-11-25T10:00:00Z";

/// bob creates W with the migrations of a new workspace (store_ensure_notes.rs), then domain d
/// and capability c under d at positions 102 and 103.
pub(crate) fn workspace() -> Workspace {
    let mut w = Workspace::default();
    apply_inner(
        &mut w,
        "bob",
        &Command::Workspace { title: "W".into() },
        1,
        AT,
    )
    .unwrap();
    let migrations = [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_task_claims",
        "migrate_persistent_claims",
        "migrate_consumers",
        "migrate_consumer_drops",
        "migrate_test_rules",
        "migrate_note_parents",
        "migrate_body",
        "migrate_purpose",
        "migrate_test_nodes",
        "migrate_generic_nodes",
        "migrate_decision_required",
    ];
    for (n, op) in migrations.into_iter().enumerate() {
        let command: Command = serde_json::from_value(json!({"op": op})).unwrap();
        crate::commands::apply_replay(&mut w, "bob", &command, n as u64 + 2, AT).unwrap();
    }
    let d = save(
        "domain",
        "d",
        None,
        0,
        json!({"title": "D", "owner": "bob", "body": "a"}),
    );
    apply(&mut w, "bob", &d, 102, AT).unwrap();
    let c = save(
        "capability",
        "c",
        Some("domain:d"),
        0,
        json!({"title": "C", "owner": "bob"}),
    );
    apply(&mut w, "bob", &c, 103, AT).unwrap();
    w
}

pub(crate) fn save(
    kind: &str,
    id: &str,
    parent: Option<&str>,
    revision: u32,
    data: Value,
) -> Command {
    serde_json::from_value(json!({"op": "save_object", "kind": kind, "id": id,
        "parent": parent, "revision": revision, "data": data, "reason": "r"}))
    .unwrap()
}

fn stamp_of(w: &Workspace) -> Option<String> {
    w.objects["capability:c"].current().parent_hash.clone()
}

#[test]
fn a_save_stamps_the_parent_hash_and_a_parent_edit_changes_the_current_hash() {
    let mut w = workspace();
    let first = contract_hash(&w, "domain:d");
    assert!(first.as_ref().is_some_and(|h| h.len() == 64), "{first:?}");
    assert_eq!(stamp_of(&w), first);
    assert_eq!(parent_hash_current(&w, "capability:c"), first);
    let d = save(
        "domain",
        "d",
        None,
        1,
        json!({"title": "D", "owner": "bob", "body": "b"}),
    );
    apply(&mut w, "bob", &d, 104, AT).unwrap();
    assert_ne!(parent_hash_current(&w, "capability:c"), first);
    assert_eq!(stamp_of(&w), first);
}

#[test]
fn an_owner_change_of_the_parent_keeps_its_hash_and_a_quiet_child_save_keeps_the_stamp() {
    let mut w = workspace();
    let first = stamp_of(&w);
    w.members.insert("ann".into(), "human".into());
    let d = save(
        "domain",
        "d",
        None,
        1,
        json!({"title": "D", "owner": "ann", "body": "a"}),
    );
    apply(&mut w, "bob", &d, 104, AT).unwrap();
    assert_eq!(contract_hash(&w, "domain:d"), first);
    let d = save(
        "domain",
        "d",
        None,
        2,
        json!({"title": "D", "owner": "ann", "body": "b"}),
    );
    apply(&mut w, "bob", &d, 105, AT).unwrap();
    let c = save(
        "capability",
        "c",
        Some("domain:d"),
        1,
        json!({"title": "C", "owner": "ann"}),
    );
    apply(&mut w, "bob", &c, 106, AT).unwrap();
    assert_eq!(w.objects["capability:c"].current().revision, 2);
    assert_eq!(stamp_of(&w), first);
    // An empty save stays a pure revision check: no revision and no new stamp.
    let empty = save(
        "capability",
        "c",
        Some("domain:d"),
        2,
        json!({"title": "C", "owner": "ann"}),
    );
    assert!(unchanged_save(&w, "bob", &empty, 107, AT).unwrap());
}

#[test]
fn a_confirmation_with_the_current_hash_stamps_it_and_other_values_are_refused() {
    let mut w = workspace();
    let d = save(
        "domain",
        "d",
        None,
        1,
        json!({"title": "D", "owner": "bob", "body": "b"}),
    );
    apply(&mut w, "bob", &d, 104, AT).unwrap();
    let current = parent_hash_current(&w, "capability:c").unwrap();
    let data = |hash: Value| json!({"title": "C", "owner": "bob", "parent_hash": hash});
    let refused = "parent_hash does not match the current parent contract";
    for wrong in [json!("abc"), json!(null), json!(7), json!(stamp_of(&w))] {
        let c = save("capability", "c", Some("domain:d"), 1, data(wrong));
        assert_eq!(
            apply(&mut w, "bob", &c, 105, AT).unwrap_err().to_string(),
            refused
        );
    }
    for field in ["parent_hash_current", "parent_changed"] {
        let mut sent = json!({"title": "C", "owner": "bob"});
        sent[field] = json!(true);
        let c = save("capability", "c", Some("domain:d"), 1, sent);
        let error = apply(&mut w, "bob", &c, 105, AT).unwrap_err().to_string();
        assert_eq!(error, format!("{field} is computed by the server"));
    }
    let c = save("capability", "c", Some("domain:d"), 1, data(json!(current)));
    assert!(!unchanged_save(&w, "bob", &c, 105, AT).unwrap());
    apply(&mut w, "bob", &c, 105, AT).unwrap();
    let r = w.objects["capability:c"].current();
    assert_eq!(
        (r.revision, r.parent_hash.as_deref()),
        (2, Some(current.as_str()))
    );
    assert!(r.data.get("parent_hash").is_none(), "{}", r.data);
    // The same confirmation again changes nothing.
    let again = save("capability", "c", Some("domain:d"), 2, data(json!(current)));
    assert!(unchanged_save(&w, "bob", &again, 106, AT).unwrap());
}

#[test]
fn equal_values_in_another_key_order_give_one_text() {
    let (mut a, mut b) = (String::new(), String::new());
    canonical(&json!({"b": [1, {"y": 2, "x": "é"}], "a": null}), &mut a);
    canonical(&json!({"a": null, "b": [1, {"x": "é", "y": 2}]}), &mut b);
    assert_eq!(a, b);
    assert_eq!(a, r#"{"a":null,"b":[1,{"x":"é","y":2}]}"#);
}

#[test]
fn a_move_stamps_the_hash_of_the_new_parent() {
    let mut w = workspace();
    let e = save(
        "domain",
        "e",
        None,
        0,
        json!({"title": "E", "owner": "bob", "body": "e"}),
    );
    apply(&mut w, "bob", &e, 104, AT).unwrap();
    let c = save(
        "capability",
        "c",
        Some("domain:e"),
        1,
        json!({"title": "C", "owner": "bob"}),
    );
    apply(&mut w, "bob", &c, 105, AT).unwrap();
    assert_eq!(stamp_of(&w), contract_hash(&w, "domain:e"));
    assert_ne!(stamp_of(&w), contract_hash(&w, "domain:d"));
}

#[test]
fn an_archived_node_has_no_contract_hash() {
    let mut w = workspace();
    let e = save(
        "domain",
        "e",
        None,
        0,
        json!({"title": "E", "owner": "bob", "body": "e"}),
    );
    apply(&mut w, "bob", &e, 104, AT).unwrap();
    assert!(contract_hash(&w, "domain:e").is_some());
    let archive: Command = serde_json::from_value(
        json!({"op": "archive_object", "target": "domain:e", "revision": 1, "reason": "r"}),
    )
    .unwrap();
    apply(&mut w, "bob", &archive, 105, AT).unwrap();
    assert_eq!(contract_hash(&w, "domain:e"), None);
}
