// Part 4 of tests/schema_objects.rs, included there, so the test names stay unchanged.

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
