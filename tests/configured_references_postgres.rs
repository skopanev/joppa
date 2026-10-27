use anyhow::{Result, ensure};
use joppa::{
    objects::{self, ReferenceRule},
    store,
};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_references_drive_queue_and_index_without_rewriting_history() -> Result<()> {
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
    types.get_mut("task").unwrap().parent_gate = None;
    types.get_mut("task").unwrap().parent_decision = false;
    types.get_mut("task").unwrap().references = [(
        "affected".into(),
        ReferenceRule {
            target: "consumer".into(),
            title: "Affects".into(),
            required: true,
            multiple: true,
            verification_subject: true,
            allow_all: false,
        },
    )]
    .into();
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Use configured context"})).await?;
    for (kind, id) in [
        ("domain", "root"),
        ("consumer", "normal"),
        ("consumer", "urgent"),
    ] {
        store::execute_remote_input(&pool,&ws,"agent",id,&json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner"},"reason":"Create queue context"})).await?;
    }
    for (id, affected) in [("first", "normal"), ("second", "urgent")] {
        store::execute_remote_input(&pool,&ws,"agent",id,&json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":"domain:root","data":{"title":id,"owner":"owner","contract":"Implement the agreed context","purpose":"implementation","affected":[affected]},"reason":"Create work"})).await?;
    }
    let priorities = vec!["consumer:urgent".into(), "consumer:normal".into()];
    let queue = store::task_queue_filtered(
        &pool,
        &ws,
        "agent",
        "available",
        100,
        None,
        None,
        None,
        &priorities,
    )
    .await?;
    assert_eq!(queue["tasks"][0]["address"], "task:second");
    for filter in ["urgent", "consumer:urgent"] {
        let filtered = store::task_queue_filtered(
            &pool,
            &ws,
            "agent",
            "available",
            100,
            None,
            Some(filter),
            None,
            &[],
        )
        .await?;
        assert_eq!(filtered["total"], 1);
        assert_eq!(filtered["tasks"][0]["address"], "task:second");
    }
    let linked = store::read_objects(
        &pool,
        &ws,
        "agent",
        &store::ObjectScope {
            related_to: Some("consumer:urgent"),
            parent: Some("*"),
            limit: 100,
            ..Default::default()
        },
    )
    .await?;
    assert!(
        linked["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["address"] == "task:second")
    );
    let before:Vec<(String,i32,Value)>=pool.get().await?.query("SELECT address,revision,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|r|Ok((r.get(0),r.get(1),serde_json::from_str(r.get::<_,&str>(2))?))).collect::<Result<_>>()?;
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["task"]["references"] = json!({});
    store::execute_remote_input(&pool,&ws,"owner","remove-rules",&json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Remove reference meaning"})).await?;
    let after:Vec<(String,i32,Value)>=pool.get().await?.query("SELECT address,revision,document::text FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|r|Ok((r.get(0),r.get(1),serde_json::from_str(r.get::<_,&str>(2))?))).collect::<Result<_>>()?;
    assert_eq!(before, after);
    assert_eq!(
        store::task_queue_filtered(
            &pool,
            &ws,
            "agent",
            "available",
            100,
            None,
            None,
            None,
            &priorities
        )
        .await?["tasks"][0]["address"],
        "task:first"
    );
    assert_eq!(pool.get().await?.query_one("SELECT count(*) FROM joppa.object_links WHERE workspace=$1 AND relation LIKE 'reference:%'",&[&ws]).await?.get::<_,i64>(0),0);
    let conn = pool.get().await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    drop(conn);
    store::migrate(&pool).await?;
    let replay = store::task_queue_filtered(
        &pool,
        &ws,
        "agent",
        "available",
        100,
        None,
        None,
        None,
        &priorities,
    )
    .await?;
    assert_eq!(replay["tasks"][0]["address"], "task:first");
    Ok(())
}
