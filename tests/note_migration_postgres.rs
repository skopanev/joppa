use anyhow::Result;
use joppa::store::{self, ObjectScope, Scope};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn source_notes_keep_public_ids_history_comments_and_files() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let ws = format!("note-migration-{}", Uuid::new_v4());
    let said = "  Дословная заметка\n\nkeep all spaces  ";
    let commands = [
        json!({"op":"workspace","title":"Migration test"}),
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
        json!({"op":"intake","id":"source","said":said,"author":"customer"}),
        json!({"op":"split","id":"source","points":[said]}),
        json!({"op":"note","target":"point:source/source.1","id":"context","text":"Original discussion"}),
        json!({"op":"attach","target":"point:source/source.1","id":"file","name":"evidence.txt","content_type":"text/plain","size":3,"hash":"b".repeat(64)}),
    ];
    let conn = pool.get().await?;
    conn.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&ws])
        .await?;
    // This journal is written by the current rules, so every replay epoch starts at 1.
    for rule in joppa::commands::REPLAY_RULES {
        conn.execute(
            "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) VALUES($1,$2,1)",
            &[&ws, &rule],
        )
        .await?;
    }
    for (index, command) in commands.iter().enumerate() {
        let actor = if index < 2 { "owner" } else { "agent" };
        conn.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb)", &[&ws,&((index+1) as i64),&actor,&Uuid::new_v4().to_string(),&command.to_string()]).await?;
    }
    drop(conn);
    store::backfill_board(&pool).await?;
    let old = store::read_objects(
        &pool,
        &ws,
        "owner",
        &ObjectScope {
            address: Some("point:source/source.1"),
            ..Default::default()
        },
    )
    .await?;
    let public = old["item_id"].as_str().unwrap().to_owned();
    let original: Value=serde_json::from_str(pool.get().await?.query_one("SELECT jsonb_object_agg(address||'/'||revision,document)::text FROM joppa.object_revisions WHERE workspace=$1", &[&ws]).await?.get(0))?;
    store::migrate_notes(&pool).await?;
    assert_eq!(
        pool.get().await?.query_one(
            "SELECT count(*) FROM joppa.objects WHERE workspace=$1 AND present AND kind IN ('intake','point')",
            &[&ws],
        ).await?.get::<_, i64>(0),
        0
    );
    let schema = store::tree_schema(&pool, &ws, "owner", None).await?;
    assert!(schema["schema"]["types"].get("intake").is_none());
    assert!(schema["schema"]["types"].get("point").is_none());
    let roots = store::read_objects(
        &pool,
        &ws,
        "owner",
        &ObjectScope {
            parent: Some("*"),
            kind: Some("note"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(roots["items"].as_array().unwrap().len(), 2);
    assert!(
        roots["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["present"] == true)
    );
    let migrated =
        store::read_item_scoped(&pool, &public, "owner", Scope::from_args(&json!({}))?).await?;
    assert_eq!(migrated["kind"], "note");
    assert_eq!(migrated["selection"]["kind"], "note");
    assert_eq!(migrated["item_id"], public);
    assert_eq!(migrated["document"]["data"]["body"], said);
    assert_eq!(migrated["document"]["data"]["author"], "customer");
    assert_eq!(
        migrated["annotations"]["comments"]["context"]["text"],
        "Original discussion"
    );
    assert_eq!(
        migrated["annotations"]["attachments"]["file"]["hash"],
        "b".repeat(64)
    );
    let canonical = migrated["address"].as_str().unwrap().to_owned();
    let children = store::read_objects(
        &pool,
        &ws,
        "owner",
        &ObjectScope {
            parent: Some("intake:source"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(children["items"][0]["address"], canonical);
    assert_eq!(children["items"][0]["item_id"], public);
    let history = store::read_item_scoped(
        &pool,
        &public,
        "owner",
        Scope::from_args(&json!({"history":true}))?,
    )
    .await?;
    assert_eq!(history["selection"]["kind"], "note");
    let old_revision = store::read_item_scoped(
        &pool,
        &public,
        "owner",
        Scope::from_args(&json!({"revision":1}))?,
    )
    .await?;
    assert_eq!(old_revision["document"]["data"]["title"], said);
    assert_eq!(old_revision["annotations"], json!({}));
    let found = store::search::find(&pool, "owner", &ws, &public, Some("note"), 10, None).await?;
    assert_eq!(found["items"][0]["system_id"], public);
    assert_eq!(found["items"][0]["kind"], "note");
    let id = migrated["id"].as_str().unwrap();
    let mut data = migrated["document"]["data"].clone();
    data["body"] = json!("Updated source");
    let edited=store::execute_remote_input(&pool,&ws,"owner","edit-note",&json!({"op":"save_object","kind":"note","id":id,"revision":migrated["current_revision"],"parent":migrated["document"]["parent"],"data":data,"links":migrated["document"]["links"],"reason":"Correct the source"})).await?;
    assert_eq!(edited["system_ids"]["note"][id], public);
    store::execute_remote_input(&pool,&ws,"agent","reply-old-address",&json!({"op":"note","target":"point:source/source.1","id":"followup","text":"New discussion"})).await?;
    let current = store::read_objects(
        &pool,
        &ws,
        "owner",
        &ObjectScope {
            address: Some(&canonical),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        current["annotations"]["comments"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    let state = store::read(&pool, &ws, "owner", &Scope::full()).await?;
    assert_eq!(state["state"]["intake"], json!({}));
    assert_eq!(state["state"]["questions"], json!({}));
    let conn = pool.get().await?;
    for (key, document) in original.as_object().unwrap() {
        let (address, revision) = key.rsplit_once('/').unwrap();
        let stored: Value=serde_json::from_str(conn.query_one("SELECT document::text FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 AND revision=$3", &[&ws,&address,&revision.parse::<i32>()?]).await?.get(0))?;
        assert_eq!(&stored, document);
    }
    let position: i64 = conn
        .query_one(
            "SELECT MAX(position) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    drop(conn);
    store::migrate_notes(&pool).await?;
    assert_eq!(
        pool.get()
            .await?
            .query_one(
                "SELECT MAX(position) FROM joppa.events WHERE workspace=$1",
                &[&ws]
            )
            .await?
            .get::<_, i64>(0),
        position
    );
    assert!(
        store::execute_remote_input(
            &pool,
            &ws,
            "owner",
            "old-create",
            &json!({"op":"intake","id":"new","said":"New information","author":"customer"})
        )
        .await
        .is_err()
    );
    Ok(())
}
