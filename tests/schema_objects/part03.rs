// Part 3 of tests/schema_objects.rs, included there, so the test names stay unchanged.

#[test]
fn explicit_legacy_grouping_changes_do_not_depend_on_prior_criterion_edits() {
    for edit_first in [false, true] {
        let mut w = World::new();
        migrate(&mut w);
        if edit_first {
            let mut data = w.state.objects["ac:ios"].current().data.clone();
            data["title"] = json!("The iOS client follows the access contract");
            save(&mut w, "ac:ios", Some("requirement:r"), data);
        }
        w.legacy("agent",json!({"op":"capability","id":"other","domain":"d","title":"Other grouping","owner":"owner"}));
        let current = w.state.requirements["r"].revisions.last().unwrap().clone();
        let mut acs = current.acs.clone();
        for ac in &mut acs {
            ac.assessment = None;
        }
        w.legacy("agent",json!({"op":"revise","req":"r","expected_revision":current.number,"capability":"other","consumers":current.consumers,"title":current.title,"body":current.body,"owner":current.owner,"reason":"Move explicit legacy grouping","acs":acs,"coverage_policy":true,"carry_tasks":true}));
        assert_eq!(
            w.state.objects["requirement:r"].current().parent.as_deref(),
            Some("capability:other")
        );
        assert!(!w.state.objects["requirement:r"].parent_override);
    }
}

#[test]
fn criterion_edits_preserve_custom_specification_type_context() {
    let mut w = empty();
    let types = json!({
        "area":{"title":"Area","role":"group","status":"children","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]}
    });
    w.ok("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Configure named roles"}));
    save(
        &mut w,
        "area:outer",
        None,
        json!({"title":"Outer","owner":"owner"}),
    );
    save(
        &mut w,
        "area:inner",
        Some("area:outer"),
        json!({"title":"Inner","owner":"owner"}),
    );
    save(
        &mut w,
        "spec:r",
        Some("area:inner"),
        json!({"title":"Access","body":"Use the access contract","owner":"owner","custom_context":{"implementation":"Retain this context"}}),
    );
    save(
        &mut w,
        "criterion:check",
        Some("spec:r"),
        json!({"title":"The access contract applies","owner":"owner"}),
    );
    let before = w.state.objects["spec:r"].current().clone();
    save(
        &mut w,
        "criterion:check",
        Some("spec:r"),
        json!({"title":"The current access contract applies","owner":"owner"}),
    );
    assert_eq!(objects::requirement_address("spec:r"), "spec:r");
    assert_eq!(w.state.objects["spec:r"].current().parent, before.parent);
    assert_eq!(w.state.objects["spec:r"].current().data, before.data);
    assert_eq!(
        w.state.objects["criterion:check"]
            .current()
            .parent
            .as_deref(),
        Some("spec:r")
    );
    assert!(
        objects::criterion_context(&w.state, "criterion:check")
            .is_some_and(|(req, _, _)| req == "spec:r")
    );
}

// [ac:product-tree-shape-ac6]
#[test]
fn verification_uses_roles_when_former_group_names_are_verification_types() {
    let mut w = empty();
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":{
        "consumer":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":["*"]},
        "domain":{"title":"Criterion","role":"ac","status":"tests","parents":["*"]},
        "capability":{"title":"Work","role":"task","status":"none","parents":["*"]}
    },"reason":"Select workflows by roles"}),
    );
    save(
        &mut w,
        "consumer:same",
        None,
        json!({"title":"Specification","owner":"owner","body":"A clear contract","consumers":{"ordinary":"data"}}),
    );
    save(
        &mut w,
        "domain:same",
        Some("consumer:same"),
        json!({"title":"Observable result","owner":"owner"}),
    );
    assert_eq!(
        objects::criterion_context(&w.state, "domain:same")
            .unwrap()
            .0,
        "consumer:same"
    );
    assert_eq!(
        w.state.objects["consumer:same"].current().data["consumers"],
        json!({"ordinary":"data"})
    );
    let number = w.state.requirements["consumer:same"]
        .revisions
        .last()
        .unwrap()
        .number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":"consumer:same","revision":number}),
    );
    assert_eq!(
        w.state.objects["consumer:same"].current().data["consumers"],
        json!({"ordinary":"data"})
    );
    save(
        &mut w,
        "capability:work",
        Some("domain:same"),
        json!({"title":"Implement","owner":"owner","contract":"Implement the exact observable result","purpose":"implementation","projects":"ordinary field"}),
    );
    assert!(w.state.task_process.contains_key("capability:work"));
    assert_eq!(
        w.state.objects["capability:work"].current().data["projects"],
        "ordinary field"
    );
    assert!(w.state.domains.is_empty());
    assert!(w.state.capabilities.is_empty());
    assert!(w.state.consumers.is_empty());
    save(
        &mut w,
        "consumer:same",
        None,
        json!({"title":"Updated specification","owner":"owner","body":"A changed contract","consumers":{"ordinary":"updated"}}),
    );
    assert_eq!(
        w.state.objects["consumer:same"].current().data["consumers"],
        json!({"ordinary":"updated"})
    );
    assert_eq!(
        w.state.requirements["consumer:same"]
            .revisions
            .last()
            .unwrap()
            .body,
        "A changed contract"
    );
}

// [product-tree-shape-ac4]
#[test]
fn every_type_can_order_its_own_siblings() {
    let mut w = empty();
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":{
        "root":{"title":"Root","role":"group","status":"children","root":true},
        "entry":{"title":"Entry","role":"document","status":"none","parents":["root"]}
    },"reason":"Configure arbitrary nesting"}),
    );
    save(
        &mut w,
        "root:area",
        None,
        json!({"title":"Area","owner":"owner"}),
    );
    for id in ["a", "b"] {
        save(
            &mut w,
            &format!("entry:{id}"),
            Some("root:area"),
            json!({"title":id,"owner":"owner"}),
        );
    }
    assert_eq!(
        tree_order::ordered_ids(&w.state, "entry", Some("root:area")).unwrap(),
        vec!["a", "b"]
    );
    w.ok("owner",json!({"op":"reorder_tree","kind":"entry","parent":"root:area","expected":["a","b"],"ids":["b","a"]}));
    assert_eq!(
        tree_order::ordered_ids(&w.state, "entry", Some("root:area")).unwrap(),
        vec!["b", "a"]
    );
}
