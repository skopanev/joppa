mod common;
#[path = "common/resolve_id.rs"]
mod resolve_id;
use common::*;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects, tasks, tree_order,
};
use serde_json::{Value, json};

fn empty() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok("owner", json!({"op":"workspace","title":"Common objects"}));
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    );
    migrate(&mut w);
    w
}
fn migrate(w: &mut World) {
    for value in [
        json!({"op":"migrate_notes"}),
        json!({"op":"migrate_tasks"}),
        json!({"op":"migrate_schema"}),
        json!({"op":"migrate_documents"}),
    ] {
        let cmd: Command = serde_json::from_value(value).unwrap();
        commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
        w.position += 1;
    }
}
fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Store the common document"}));
}

#[test]
fn required_fields_follow_configured_types_and_value_presence() {
    let mut w = empty();
    let mut types = objects::schema(&w.state).types;
    let names = ["domain", "ac", "task", "note", "feedback"];
    for name in names {
        types.insert(name.into(), serde_json::from_value(json!({"title":name,"role":"document","status":"none","root":true,"parents":["*"],"required_fields":{"origin":"origin is required"}})).unwrap());
    }
    types.insert(
        "record".into(),
        serde_json::from_value(
            json!({"title":"Record","role":"document","status":"none","root":true,"parents":["*"]}),
        )
        .unwrap(),
    );
    w.ok("owner", json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Require an ordinary field on configured types"}));
    assert_eq!(
        serde_json::to_value(&objects::schema(&w.state).types["note"].required_fields).unwrap(),
        json!({"origin":"origin is required"})
    );
    for name in names {
        for value in [
            None,
            Some(Value::Null),
            Some(json!("")),
            Some(json!(" \t\n ")),
            Some(json!([])),
            Some(json!({})),
        ] {
            let mut data = json!({"title":"A document","owner":"owner"});
            if let Some(value) = value {
                data["origin"] = value;
            }
            let before = serde_json::to_value(&w.state).unwrap();
            let error = w.apply("agent", json!({"op":"save_object","kind":name,"id":"empty","revision":0,"parent":null,"data":data,"reason":"Reject an absent value"})).unwrap_err();
            assert_eq!(error.to_string(), "origin is required");
            assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
        }
        for (index, value) in [
            json!(0),
            json!(false),
            json!("origin"),
            json!([0]),
            json!({"value":false}),
        ]
        .into_iter()
        .enumerate()
        {
            let key = format!("{name}:value-{index}");
            save(
                &mut w,
                &key,
                None,
                json!({"title":"A document","owner":"owner","origin":value}),
            );
            assert_eq!(w.state.objects[&key].current().data["origin"], value);
        }
    }
    save(
        &mut w,
        "record:plain",
        None,
        json!({"title":"An ordinary document","owner":"owner"}),
    );
}

#[test]
fn required_field_rules_reject_invalid_configuration_and_default_to_empty() {
    let mut w = empty();
    let current = objects::schema(&w.state);
    for t in current.types.values() {
        assert!(t.required_fields.is_empty());
        assert!(
            serde_json::to_value(t)
                .unwrap()
                .get("required_fields")
                .is_none()
        );
    }
    for rule in [
        json!({"bad field":"Value is required"}),
        json!({"source":" \n "}),
        json!({"verification":"Verification is required"}),
        json!({"text":{"contains":[],"error":"Text is required"}}),
        json!({"text":{"contains":[" \n "],"error":"Text is required"}}),
        json!({"text":{"contains":["marker"],"error":" \n "}}),
        json!({"text":{"contains":["marker"]}}),
        json!({"text":{"contains":["marker"],"error":"Text is required","extra":true}}),
    ] {
        let mut types = serde_json::to_value(&current.types).unwrap();
        types["note"]["required_fields"] = rule;
        let before = serde_json::to_value(&w.state).unwrap();
        w.no("owner", json!({"op":"set_tree_schema","revision":current.revision,"types":types,"reason":"Reject an invalid required field rule"}));
        assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
    }
}

#[test]
fn required_text_rules_use_literal_markers_on_any_document_field() {
    let mut w = empty();
    save(
        &mut w,
        "note:old",
        None,
        json!({"title":"Old input","owner":"owner","body":"Original text"}),
    );
    let original = w.state.objects["note:old"].current().clone();
    w.ok("owner", json!({"op":"archive_object","target":"note:old","revision":1,"reason":"Archive the original input"}));
    let mut types = serde_json::to_value(objects::schema(&w.state).types).unwrap();
    let rule = json!({"contains":["left:","Right:"],"error":"Both markers are required"});
    types["note"]["required_fields"] = json!({"body":rule});
    types["observation"] = json!({"title":"Observation","role":"document","status":"none","root":true,"parents":["*"],"required_fields":{"text":rule}});
    w.ok("owner", json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Require configured text markers"}));
    assert_eq!(
        serde_json::to_value(
            objects::schema(&w.state).types["observation"]
                .required_fields
                .clone()
        )
        .unwrap(),
        json!({"text":rule})
    );
    for (kind, field) in [("note", "body"), ("observation", "text")] {
        for invalid in [
            Value::Null,
            json!(false),
            json!(1),
            json!([]),
            json!({"left:":"Right:"}),
            json!(""),
            json!("left:"),
            json!("Right:"),
            json!("Left: Right:"),
            json!("left: right:"),
        ] {
            let mut data = json!({"title":"An observation","owner":"owner"});
            data[field] = invalid;
            let before = serde_json::to_value(&w.state).unwrap();
            let error = w.apply("agent", json!({"op":"save_object","kind":kind,"id":"new","revision":0,"parent":null,"data":data,"reason":"Reject invalid text"})).unwrap_err();
            assert_eq!(error.to_string(), "Both markers are required");
            assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
        }
        let mut data = json!({"title":"An observation","owner":"owner","status":"unprocessed"});
        data[field] = json!("Right: then left:");
        let key = format!("{kind}:new");
        save(&mut w, &key, None, data.clone());
        assert_eq!(w.state.objects[&key].current().data, data);
        data[field] = json!("left: only");
        let error = w.apply("agent", json!({"op":"save_object","kind":kind,"id":"new","revision":1,"parent":null,"data":data,"reason":"Reject removal of a marker"})).unwrap_err();
        assert_eq!(error.to_string(), "Both markers are required");
        assert_eq!(w.state.objects[&key].current().revision, 1);
    }
    assert_eq!(
        serde_json::to_value(&w.state.objects["note:old"].revisions[0]).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    assert!(!w.state.objects["note:old"].current().present);
}

// [product-tree-shape-ac1]
#[test]
fn every_former_type_name_obeys_the_schema_with_identical_document_fields() {
    let mut w = empty();
    let mut types = serde_json::Map::new();
    let names = [
        "domain",
        "capability",
        "consumer",
        "requirement",
        "ac",
        "task",
        "project",
        "note",
        "intake",
        "point",
        "custom",
    ];
    for name in names {
        types.insert(
            name.into(),
            json!({"title":name,"role":"document","status":"none","root":true,"parents":["*"]}),
        );
    }
    w.ok("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Names do not select behavior"}));
    let data = json!({"title":"The same document","owner":"owner","body":"Common text","description":"Common description","consumers":{"ordinary":"data"},"projects":"ordinary data","status":"ordinary"});
    for name in names {
        let key = format!("{name}:same");
        save(&mut w, &key, None, data.clone());
        assert_eq!(w.state.objects[&key].current().data, data);
        assert!(!objects::input_type(&w.state, name));
        w.legacy(
            "agent",
            json!({"op":"describe","target":key,"text":"Same annotation","reason":"Add context"}),
        );
        assert_eq!(w.state.notes[&key].description, "Same annotation");
        assert_eq!(w.state.objects[&key].current().revision, 1);
    }
    assert!(w.state.domains.is_empty());
    assert!(w.state.capabilities.is_empty());
    assert!(w.state.consumers.is_empty());
    assert!(w.state.requirements.is_empty());
    assert!(w.state.task_process.is_empty());
    let mut types = objects::schema(&w.state).types;
    types.get_mut("custom").unwrap().inbox = Some(true);
    w.ok("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Enable only the configured input queue"}));
    save(
        &mut w,
        "custom:input",
        None,
        json!({"title":"Incoming fact","owner":"owner"}),
    );
    w.ok("owner",json!({"op":"decide_note","target":"custom:input","revision":1,"outcome":"rejected","reason":"Reviewed the fact"}));
    assert_eq!(
        objects::process(&w.state, "custom:input")["status"],
        "processed"
    );
    assert!(!objects::input_type(&w.state, "note"));
}

// [ac:restructure-keeps-evidence-ac4]
#[test]
fn document_migration_preserves_all_revisions_and_runs_once() {
    let mut w = World::new();
    objects::migrate_notes(&mut w.state, "owner", w.position + 1, "now").unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 2, "now").unwrap();
    objects::migrate_schema(&mut w.state, "owner", w.position + 3, "now").unwrap();
    let old = serde_json::to_value(&w.state.objects).unwrap();
    objects::migrate_documents(&mut w.state, "owner", w.position + 4, "now").unwrap();
    assert_eq!(serde_json::to_value(&w.state.objects).unwrap(), old);
    let current = serde_json::to_value(&w.state).unwrap();
    objects::migrate_documents(&mut w.state, "owner", w.position + 5, "later").unwrap();
    assert_eq!(serde_json::to_value(&w.state).unwrap(), current);
    let mut data = w.state.objects["domain:d"].current().data.clone();
    data["extra"] = json!({"preserved":true});
    save(&mut w, "domain:d", None, data.clone());
    assert_eq!(w.state.objects["domain:d"].current().data, data);
    assert_eq!(
        w.state.objects["domain:d"].revisions[0].data,
        old["domain:d"]["revisions"][0]["data"]
    );
}

#[test]
fn migrated_criterion_edits_preserve_native_and_custom_specification_parents() {
    for grouping in ["native", "moved", "custom"] {
        let custom = grouping == "custom";
        let mut w = World::new();
        migrate(&mut w);
        let mut types = objects::schema(&w.state).types;
        types.get_mut("requirement").unwrap().references.insert(
            "audiences".into(),
            objects::ReferenceRule {
                target: "domain".into(),
                title: "Audience".into(),
                multiple: true,
                required: true,
                verification_subject: false,
                allow_all: false,
            },
        );
        if custom {
            types.insert(
                "section".into(),
                serde_json::from_value(
                    json!({"title":"Section","role":"document","status":"none","root":true,"parents":["*"]}),
                )
                .unwrap(),
            );
            types
                .get_mut("requirement")
                .unwrap()
                .parents
                .push("section".into());
        }
        // Add the configured reference to the imported document before validation.
        w.state
            .objects
            .get_mut("requirement:r")
            .unwrap()
            .revisions
            .last_mut()
            .unwrap()
            .data["audiences"] = json!(["d"]);
        w.ok("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Configure specification context"}));
        w.state
            .objects
            .get_mut("requirement:r")
            .unwrap()
            .revisions
            .last_mut()
            .unwrap()
            .data["custom_context"] = json!({"implementation":"Keep this context"});
        if custom {
            save(
                &mut w,
                "section:outer",
                None,
                json!({"title":"Outer","owner":"owner"}),
            );
            save(
                &mut w,
                "section:inner",
                Some("section:outer"),
                json!({"title":"Inner","owner":"owner"}),
            );
            let data = w.state.objects["requirement:r"].current().data.clone();
            save(&mut w, "requirement:r", Some("section:inner"), data);
        } else if grouping == "moved" {
            save(
                &mut w,
                "capability:other",
                Some("domain:d"),
                json!({"title":"Other grouping","owner":"owner"}),
            );
            let data = w.state.objects["requirement:r"].current().data.clone();
            save(&mut w, "requirement:r", Some("capability:other"), data);
        } else {
            assert!(!w.state.objects["requirement:r"].parent_override);
        }
        let before = w.state.objects["requirement:r"].clone();
        let criterion_ids: Vec<_> = w.state.requirements["r"]
            .revisions
            .last()
            .unwrap()
            .acs
            .iter()
            .map(|ac| ac.id.clone())
            .collect();
        let mut data = w.state.objects["ac:ios"].current().data.clone();
        data["title"] = json!("The iOS client follows the current access contract");
        save(&mut w, "ac:ios", Some("requirement:r"), data);
        let after = w.state.objects["requirement:r"].current();
        assert_eq!(after.parent, before.current().parent);
        assert_eq!(
            w.state.objects["requirement:r"].parent_override,
            before.parent_override
        );
        assert_eq!(after.data, before.current().data);
        assert_eq!(after.links, before.current().links);
        assert_eq!(
            serde_json::to_value(
                &w.state.objects["requirement:r"].revisions[..before.revisions.len()]
            )
            .unwrap(),
            serde_json::to_value(&before.revisions).unwrap()
        );
        assert_eq!(
            w.state.requirements["r"]
                .revisions
                .last()
                .unwrap()
                .acs
                .iter()
                .map(|ac| ac.id.clone())
                .collect::<Vec<_>>(),
            criterion_ids
        );
        assert!(objects::referenced(&w.state, "ac:ios", "domain:d"));
        let current = w.state.objects["ac:ios"].current().clone();
        let state = serde_json::to_value(&w.state).unwrap();
        let position = w.position;
        for (revision, parent) in [
            (current.revision - 1, Some("requirement:r")),
            (current.revision, None),
            (current.revision, Some("domain:d")),
        ] {
            w.no("agent",json!({"op":"save_object","kind":"ac","id":"ios","revision":revision,"parent":parent,"data":current.data,"links":current.links,"reason":"Reject stale or invalid context"}));
            assert_eq!(serde_json::to_value(&w.state).unwrap(), state);
            assert_eq!(w.position, position);
        }
        let parent = w.state.objects["requirement:r"]
            .current()
            .parent
            .clone()
            .unwrap();
        w.no("owner",json!({"op":"archive_object","target":parent,"revision":w.state.objects[&parent].current().revision,"reason":"Reject archive with a present specification"}));
        let parent_kind = w.state.objects[&parent].kind.clone();
        let mut invalid_schema = objects::schema(&w.state).types;
        invalid_schema.remove(&parent_kind);
        w.no("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":invalid_schema,"reason":"Reject removal of a used parent type"}));
        assert_eq!(serde_json::to_value(&w.state).unwrap(), state);
        assert_eq!(w.position, position);
    }
}

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

// [ac:mcp-schema-objects-ac4]
#[test]
fn native_and_custom_verification_names_use_common_process_keys() {
    for (spec, criterion) in [("requirement", "ac"), ("contract", "check")] {
        let mut w = empty();
        let types = json!({spec:{"title":"Specification","role":"requirement","status":"children","root":true},
            criterion:{"title":"Criterion","role":"ac","status":"tests","parents":[spec]}});
        w.ok(
            "owner",
            json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,
            "types":types,"reason":"Use configured verification roles"}),
        );
        let key = format!("{spec}:same");
        let child = format!("{criterion}:same");
        save(
            &mut w,
            &key,
            None,
            json!({"title":"Contract","body":"Expected result","owner":"owner","consumers":"ordinary"}),
        );
        save(
            &mut w,
            &child,
            Some(&key),
            json!({"title":"Observable result","owner":"owner"}),
        );
        assert!(w.state.requirements.contains_key(&key));
        assert_eq!(
            objects::criterion_context(&w.state, &child).unwrap().2.id,
            child
        );
        save(
            &mut w,
            &key,
            None,
            json!({"title":"Contract","body":"Changed result","owner":"owner","consumers":"updated"}),
        );
        assert_eq!(w.state.objects[&key].current().data["consumers"], "updated");
        assert_eq!(w.state.requirements.len(), 1);
    }
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn copied_ids_search_and_rebuild_use_common_documents_for_all_names() -> anyhow::Result<()> {
    use joppa::store::{self, ObjectScope};
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("common-types-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "create",
        &json!({"op":"workspace","title":"Common read paths"}),
    )
    .await?;
    let schema = store::tree_schema(&pool, &ws, "owner", None).await?;
    let names = [
        "domain",
        "capability",
        "consumer",
        "requirement",
        "ac",
        "task",
        "project",
        "note",
        "intake",
        "point",
        "custom",
    ];
    let types: serde_json::Map<String, Value> = names
        .iter()
        .map(|name| {
            (
                (*name).into(),
                json!({"title":name,"role":"document","status":"none","root":true,"parents":["*"]}),
            )
        })
        .collect();
    store::execute_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Configure every name as a document"})).await?;
    let data = json!({"title":"uniformneedle","owner":"owner","consumers":"ordinary field","projects":{"ordinary":true}});
    for kind in names {
        store::execute_input(&pool,&ws,"owner",kind,&json!({"op":"save_object","kind":kind,"id":"same","revision":0,"parent":null,"data":data,"reason":"Store common document"})).await?;
        let key = format!("{kind}:same");
        let object = store::read_objects(
            &pool,
            &ws,
            "owner",
            &ObjectScope {
                address: Some(&key),
                ..Default::default()
            },
        )
        .await?;
        let public = object["item_id"].as_str().unwrap();
        assert_eq!(resolve_id::resolve(&pool, "owner", &ws, public).await?, key);
        assert_eq!(object["document"]["data"], data);
        assert!(
            resolve_id::resolve(&pool, "outsider", &ws, public)
                .await
                .is_err()
        );
        let found =
            store::search::find(&pool, "owner", &ws, "uniformneedle", Some(kind), 10, None).await?;
        assert_eq!(found["indexed"], 1);
        assert_eq!(found["items"][0]["kind"], kind);
        assert_eq!(found["items"][0]["id"], "same");
    }
    assert!(
        store::read_objects(
            &pool,
            &ws,
            "owner",
            &ObjectScope {
                kind: Some("note"),
                note_status: Some("unprocessed"),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    let before=pool.get().await?.query("SELECT address,revision,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|row|(row.get::<_,String>(0),row.get::<_,i32>(1),row.get::<_,String>(2))).collect::<Vec<_>>();
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::migrate(&pool).await?;
    let after=pool.get().await?.query("SELECT address,revision,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|row|(row.get::<_,String>(0),row.get::<_,i32>(1),row.get::<_,String>(2))).collect::<Vec<_>>();
    assert_eq!(before, after);
    Ok(())
}
