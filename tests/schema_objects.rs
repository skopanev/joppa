mod common;
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
    w.ok("owner", json!({"op":"member","id":"agent","role":"agent"}));
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
            json!({"title":name,"role":"document","root":true,"parents":["*"]}),
        );
    }
    w.ok("agent",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Names do not select behavior"}));
    let data = json!({"title":"The same document","owner":"owner","body":"Common text","description":"Common description","consumers":{"ordinary":"data"},"projects":"ordinary data","status":"ordinary"});
    for name in names {
        let key = format!("{name}:same");
        save(&mut w, &key, None, data.clone());
        assert_eq!(w.state.objects[&key].current().data, data);
        assert!(!objects::input_type(&w.state, name));
        w.ok(
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
    w.ok("agent",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Enable only the configured input queue"}));
    save(
        &mut w,
        "custom:input",
        None,
        json!({"title":"Incoming fact","owner":"owner"}),
    );
    w.ok("agent",json!({"op":"set_note_status","target":"custom:input","revision":1,"status":"processed","reason":"Reviewed the fact"}));
    assert_eq!(
        objects::process(&w.state, "custom:input")["status"],
        "processed"
    );
    assert!(!objects::input_type(&w.state, "note"));
}

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
fn verification_uses_roles_when_former_group_names_are_verification_types() {
    let mut w = empty();
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":{
        "consumer":{"title":"Specification","role":"requirement","root":true,"parents":["*"]},
        "domain":{"title":"Criterion","role":"ac","parents":["*"]},
        "capability":{"title":"Work","role":"task","parents":["*"]}
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

#[test]
fn every_type_can_order_its_own_siblings() {
    let mut w = empty();
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":{
        "root":{"title":"Root","role":"group","root":true},
        "entry":{"title":"Entry","role":"document","parents":["root"]}
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

#[test]
fn native_and_custom_verification_names_use_common_process_keys() {
    for (spec, criterion) in [("requirement", "ac"), ("contract", "check")] {
        let mut w = empty();
        let types = json!({spec:{"title":"Specification","role":"requirement","root":true},
            criterion:{"title":"Criterion","role":"ac","parents":[spec]}});
        w.ok(
            "agent",
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
                json!({"title":name,"role":"document","root":true,"parents":["*"]}),
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
        let copied = store::read_item(&pool, object["item_id"].as_str().unwrap(), "owner").await?;
        assert_eq!(copied["address"], key);
        assert_eq!(copied["document"]["data"], data);
        assert!(copied.get("index").is_none());
        assert!(
            store::read_item(&pool, object["item_id"].as_str().unwrap(), "outsider")
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
