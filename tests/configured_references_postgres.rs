use anyhow::{Result, ensure};
use joppa::{
    objects::{self, ReferenceRule},
    store,
};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_references_drive_the_index_without_rewriting_history() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("references-{}", uuid::Uuid::new_v4().simple());
    for (request, command) in [
        (
            "workspace",
            json!({"op":"workspace","title":"Configured references"}),
        ),
        (
            "member",
            json!({"op":"enroll_service","id":"agent","role":"agent"}),
        ),
    ] {
        store::execute_input(&pool, &ws, "owner", request, &command).await?;
    }
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = objects::defaults();
    types.get_mut("task").unwrap().parents = vec!["domain".into()];
    types.get_mut("task").unwrap().parent_decision = false;
    types.get_mut("task").unwrap().references = [(
        "affected".into(),
        ReferenceRule {
            target: "consumer".into(),
            title: "Affects".into(),
            required: true,
            multiple: true,
            verification_subject: None,
            allow_all: false,
            from: None,
        },
    )]
    .into();
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Use configured context"})).await?;
    for (kind, id) in [
        ("domain", "root"),
        ("consumer", "normal"),
        ("consumer", "urgent"),
    ] {
        store::execute_remote_input(&pool,&ws,"agent",id,&json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner"},"reason":"Create the context"})).await?;
    }
    for (id, affected) in [("first", "normal"), ("second", "urgent")] {
        store::execute_remote_input(&pool,&ws,"agent",id,&json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":"domain:root","data":{"title":id,"owner":"owner","body":"Implement the agreed context","affected":[affected]},"reason":"Create work"})).await?;
    }
    let related = |linked: Value| -> Vec<String> {
        linked["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["address"].as_str().unwrap().to_owned())
            .collect()
    };
    let read_related = || async {
        store::read_objects(
            &pool,
            &ws,
            "agent",
            &store::ObjectScope {
                filter: joppa::node_filter::NodeFilter::of(Some("*"), None),
                related_to: Some("consumer:urgent"),
                limit: 100,
                ..Default::default()
            },
        )
        .await
    };
    assert_eq!(related(read_related().await?), ["task:second"]);
    let before:Vec<(String,i32,Value)>=pool.get().await?.query("SELECT address,revision,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|r|Ok((r.get(0),r.get(1),serde_json::from_str(r.get::<_,&str>(2))?))).collect::<Result<_>>()?;
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["task"]["references"] = json!({});
    store::execute_remote_input(&pool,&ws,"owner","remove-rules",&json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Remove reference meaning"})).await?;
    let after:Vec<(String,i32,Value)>=pool.get().await?.query("SELECT address,revision,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|r|Ok((r.get(0),r.get(1),serde_json::from_str(r.get::<_,&str>(2))?))).collect::<Result<_>>()?;
    assert_eq!(before, after);
    // Without the rule the saved value is plain data: no link, so no related object.
    assert!(related(read_related().await?).is_empty());
    assert_eq!(pool.get().await?.query_one("SELECT count(*) FROM joppa.object_links WHERE workspace=$1 AND relation LIKE 'reference:%'",&[&ws]).await?.get::<_,i64>(0),0);
    let conn = pool.get().await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    drop(conn);
    store::migrate(&pool).await?;
    assert!(related(read_related().await?).is_empty());
    Ok(())
}
