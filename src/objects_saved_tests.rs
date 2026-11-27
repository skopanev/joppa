//! The refusals of a write that names its cause: a stale revision names the address, the sent
//! and the current revision, and a save names the place of its title and body.
use super::*;
use crate::commands::{Command, apply};
use crate::objects::parent_hash::tests::{AT, save, workspace};

#[test]
fn a_stale_save_archive_and_restore_name_the_address_and_both_revisions() {
    let mut w = workspace();
    let error = |w: &mut Workspace, command: &Command| {
        apply(w, "bob", command, 110, AT).unwrap_err().to_string()
    };
    let c = save(
        "capability",
        "c",
        Some("domain:d"),
        3,
        json!({"title": "C", "owner": "bob"}),
    );
    let stale = "stale object revision on capability:c: sent 3, current is 1";
    assert_eq!(error(&mut w, &c), stale);
    let new = save(
        "capability",
        "n",
        Some("domain:d"),
        2,
        json!({"title": "N", "owner": "bob"}),
    );
    assert_eq!(
        error(&mut w, &new),
        "stale object revision on capability:n: sent 2, current is 0"
    );
    let op = |op: &str, revision: u32| -> Command {
        serde_json::from_value(
            json!({"op": op, "target": "capability:c", "revision": revision, "reason": "r"}),
        )
        .unwrap()
    };
    assert_eq!(error(&mut w, &op("archive_object", 3)), stale);
    apply(&mut w, "bob", &op("archive_object", 1), 111, AT).unwrap();
    let archived = "stale object revision on capability:c: sent 1, current is 2";
    assert_eq!(error(&mut w, &op("restore_object", 1)), archived);
    assert!(!archived.contains(';'));
}

#[test]
fn a_save_with_a_title_or_body_outside_data_names_their_place() {
    for field in ["title", "body"] {
        let mut sent = json!({"op": "save_object", "kind": "ac", "id": "x", "revision": 0,
            "reason": "r", "data": {"title": "T", "owner": "bob"}});
        sent[field] = json!("T");
        let error = crate::store::check_fields(&sent).unwrap_err().to_string();
        assert_eq!(error, "put title and body inside data");
    }
    let update = json!({"op": "update_workspace", "title": "W"});
    let error = crate::store::check_fields(&update)
        .err()
        .map(|e| e.to_string());
    assert_ne!(error.as_deref(), Some("put title and body inside data"));
}
