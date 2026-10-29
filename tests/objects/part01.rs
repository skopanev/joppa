// Part 1 of tests/objects.rs, included there, so the test names stay unchanged.

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value, links: Vec<ObjectLink>) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"links":links,"reason":"Update the current document"}));
}

fn link(target: &str, revision: Option<u32>, blocking: bool) -> ObjectLink {
    ObjectLink {
        target: target.into(),
        revision,
        blocking,
        relation: if revision.is_some() {
            "depends_on"
        } else {
            "related"
        }
        .into(),
    }
}

// [request-verbatim-ac1]
#[test]
fn note_revisions_preserve_source_and_reject_stale_writes() {
    let mut w = World::new();
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"New source","body":"Original text","owner":"owner"}),
        vec![],
    );
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"New source","body":"Updated text","owner":"owner"}),
        vec![
            link("domain:d", None, false),
            link("consumer:customer", None, false),
        ],
    );
    let o = &w.state.objects["note:n"];
    assert_eq!(o.revisions.len(), 2);
    assert_eq!(o.revisions[0].data["body"], "Original text");
    assert_eq!(o.current().data["body"], "Updated text");
    assert_eq!(o.current().links.len(), 2);
    w.no("agent",json!({"op":"save_object","kind":"note","id":"n","revision":1,"parent":null,"data":{"title":"Overwrite","owner":"owner"},"reason":"Stale edit"}));
    w.ok(
        "agent",
        json!({"op":"note","target":"note:n","id":"c","text":"Read the source"}),
    );
    assert_eq!(
        w.state.notes["note:n"].comments["c"].at,
        "2026-01-01T00:00:00Z"
    );
}

#[test]
fn schema_supports_custom_grouping_without_a_capability() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.insert(
        "initiative".into(),
        TreeType {
            question: None,
            index: None,
            references: Default::default(),
            required_fields: Default::default(),
            parent_decision: false,
            status: None,
            status_order: None,
            process_fields: None,
            inbox: None,
            launch_blocker: None,
            color: None,
            title_size: None,
            badge: None,
            title_preview_length: None,
            parent_gate: None,
            title: "Initiative".into(),
            role: "group".into(),
            root: true,
            parents: vec!["initiative".into()],
        },
    );
    types
        .get_mut("requirement")
        .unwrap()
        .parents
        .push("initiative".into());
    w.ok("owner",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Add an initiative level"}));
    save(
        &mut w,
        "initiative:i",
        None,
        json!({"title":"Release","owner":"owner"}),
        vec![],
    );
    save(
        &mut w,
        "requirement:custom",
        Some("initiative:i"),
        json!({"title":"Custom grouping","body":"Read the release","owner":"owner","consumers":["customer"]}),
        vec![],
    );
    assert_eq!(w.state.objects["requirement:custom"].revisions.len(), 1);
    save(
        &mut w,
        "ac:custom-ac",
        Some("requirement:custom"),
        json!({"title":"The release opens","owner":null,"description":"","depends_on":[],"before_launch":false}),
        vec![],
    );
    let revision = w.state.requirements["custom"]
        .revisions
        .last()
        .unwrap()
        .number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":"custom","revision":revision}),
    );
    w.ok("agent",json!({"op":"assess","req":"custom","revision":revision,"ac":"custom-ac","assessment":{"ready":true,"reason":"Contract is clear","owner":null,"decision":null}}));
    joppa::tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    save(
        &mut w,
        "task:custom/t",
        Some("ac:custom-ac"),
        json!({"title":"Implement the release","contract":"Change the release view","projects":["app"],"purpose":"implementation"}),
        vec![],
    );
    assert!(objects::within(&w.state, "task:custom/t", "initiative:i"));
    assert!(
        joppa::tasks::claimable(
            &w.state,
            "agent",
            "task:custom/t",
            &w.state.task_process["task:custom/t"]
        )
        .is_ok()
    );
    w.no("owner",json!({"op":"set_tree_schema","revision":0,"types":objects::defaults(),"reason":"Stale schema"}));
    let mut invalid = objects::schema(&w.state).types;
    invalid.get_mut("ac").unwrap().parents = vec!["initiative".into()];
    w.no(
        "owner",
        json!({"op":"set_tree_schema","revision":1,"types":invalid,"reason":"Invalid AC parent"}),
    );
}

// [ac:all-object-links-ac3]
#[test]
fn ordinary_cycles_are_allowed_and_blocking_cycles_are_rejected() {
    let mut w = World::new();
    let d = w.state.objects["domain:d"].current().data.clone();
    save(
        &mut w,
        "domain:d",
        None,
        d,
        vec![link("domain:d", None, false)],
    );
    let ios = w.state.objects["ac:ios"].current().data.clone();
    save(
        &mut w,
        "ac:ios",
        Some("requirement:r"),
        ios.clone(),
        vec![link("ac:key", Some(1), true)],
    );
    let key = w.state.objects["ac:key"].current().data.clone();
    let n = w.state.objects["ac:key"].current().revision;
    w.no("agent",json!({"op":"save_object","id":"key","kind":"ac","revision":n,"parent":"requirement:r","data":key,"links":[link("ac:ios",Some(2),true)],"reason":"Close a blocking cycle"}));
    save(
        &mut w,
        "ac:ios",
        Some("requirement:r"),
        ios,
        vec![link("ac:key", None, false)],
    );
}

#[test]
fn nonblocking_stale_dependency_flags_review_without_blocking_claim() {
    let mut w = World::new();
    w.ready();
    w.legacy("agent",json!({"op":"task","req":"r","revision":1,"id":"t","title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Change the access screen"}));
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"Context","body":"First","owner":"owner"}),
        vec![],
    );
    let ac = w.state.objects["ac:ios"].current().data.clone();
    save(
        &mut w,
        "ac:ios",
        Some("requirement:r"),
        ac.clone(),
        vec![link("note:n", Some(1), false)],
    );
    // check-list-frozen-ac2: the link change resets readiness, so assess it again.
    w.ok("agent", assess("ios", true));
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"Context","body":"Second","owner":"owner"}),
        vec![],
    );
    // dependencies-before-priority: an ordering link of a criterion pins nothing.
    assert!(objects::review_links(&w.state, "ac:ios").is_empty());
    assert!(objects::work_ready(&w.state, "ac:ios"));
    let r = w.state.requirements["r"].revisions.last().unwrap();
    assert!(joppa::checks::claimable(&w.state, "agent", "r", "t", r, &r.tasks["t"]).is_ok());
    assert_eq!(r.number, 1, "a link does not rewrite the test contract");
}
