mod common;
use common::*;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects::TreeType,
};
use serde_json::{Value, json};

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set the document contract"}));
}

fn document(title: &str) -> Value {
    json!({"title":title,"owner":"owner"})
}

fn contract(title: &str) -> Value {
    json!({"title":title,"owner":"owner","body":"Implement the specified document behavior","projects":[]})
}

fn configured() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Generic specifications"}),
    );
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    );
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"other","role":"agent"}),
    );
    let mut types: std::collections::BTreeMap<String, TreeType> = std::collections::BTreeMap::new();
    for (kind, role, root, parents) in [
        ("spec", "document", true, vec!["spec"]),
        ("task", "task", false, vec!["*"]),
        ("work", "task", true, vec!["*"]),
    ] {
        types.insert(
            kind.into(),
            TreeType {
                question: None,
                index: None,
                references: Default::default(),
                required_fields: Default::default(),
                fields: Default::default(),
                parent_decision: false,
                status: None,
                process_fields: None,
                inbox: None,
                source: false,
                launch_blocker: None,
                color: None,
                title_size: None,
                badge: None,
                title_preview_length: None,
                field_max_length: None,
                title: kind.into(),
                role: role.into(),
                root,
                parents: parents.into_iter().map(String::from).collect(),
                parent_gate: None,
            },
        );
    }
    w.ok("owner",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Use specifications with any depth"}));
    save(&mut w, "spec:root", None, document("Root specification"));
    save(
        &mut w,
        "spec:child",
        Some("spec:root"),
        document("Nested specification"),
    );
    joppa::tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    joppa::body_migration::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    w
}

// edit-makes-revision-ac3
#[test]
fn an_identical_save_keeps_the_workspace_and_a_change_adds_one_revision() {
    let mut w = configured();
    save(&mut w, "work:source", None, contract("Source task"));
    w.ok("agent",json!({"op":"save_object","kind":"task","id":"dependent","revision":0,"parent":"spec:root","data":contract("Dependent task"),"links":[{"target":"work:source","relation":"depends_on","revision":1,"blocking":true}],"reason":"Depend on the source"}));
    let before = format!("{:?}", w.state);
    save(&mut w, "work:source", None, contract("Source task"));
    assert_eq!(format!("{:?}", w.state), before);
    let mut changed = contract("Source task");
    changed["body"] = json!("Implement the changed document behavior");
    save(&mut w, "work:source", None, changed);
    assert_eq!(w.state.objects["work:source"].revisions.len(), 2);

    let linked = |revision: u32, links: Value| json!({"op":"save_object","kind":"spec","id":"child","revision":revision,"parent":"spec:root","data":document("Nested specification"),"links":links,"reason":"Relate the specification"});
    w.ok("agent", linked(1, json!([])));
    assert_eq!(w.state.objects["spec:child"].revisions.len(), 1);
    w.ok(
        "agent",
        linked(1, json!([{"target":"spec:root","relation":"related"}])),
    );
    assert_eq!(w.state.objects["spec:child"].revisions.len(), 2);

    // A stored identical save still adds its historical revision on replay.
    let cmd: Command = serde_json::from_value(linked(
        2,
        json!([{"target":"spec:root","relation":"related"}]),
    ))
    .unwrap();
    commands::apply_replay(&mut w.state, "agent", &cmd, w.position + 1, "later").unwrap();
    assert_eq!(w.state.objects["spec:child"].revisions.len(), 3);
}
