mod common;
use anyhow::Result;
use common::*;
use joppa::store;
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn storage_reads_only_selected_history_and_preserves_unrelated_input_payloads() -> Result<()>
{
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "requires a test database");
    store::migrate(&pool).await?;
    let ws = format!("storage-{}", uuid::Uuid::new_v4().simple());
    for (i, command) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{i}"), &command).await?;
    }
    let raw = "original source input ".repeat(100_000);
    store::execute_input(&pool,&ws,"agent","input",&json!({"op":"save_object","kind":"note","id":"raw","revision":0,"parent":null,"data":{"title":"Source input","owner":"owner","body":raw,"status":"unprocessed"},"links":[],"reason":"Capture source"})).await?;
    let conn = pool.get().await?;
    let row = conn.query_one("SELECT xmin::text,value->'revisions'->0->'data'->>'body',compact->'revisions'->0->'data' ? 'body' FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key='note:raw'", &[&ws]).await?;
    let original_xmin: String = row.get(0);
    assert_eq!(row.get::<_, String>(1), raw);
    assert!(
        !row.get::<_, bool>(2),
        "the validation graph must exclude input bodies"
    );
    drop(conn);
    store::execute_input(&pool, &ws, "agent", "revision-2", &revision(1)).await?;
    let conn = pool.get().await?;
    let xmin: String=conn.query_one("SELECT xmin::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key='note:raw'", &[&ws]).await?.get(0);
    assert_eq!(
        xmin, original_xmin,
        "an unrelated write must not replace the input row"
    );
    let metadata: Value = serde_json::from_str(
        conn.query_one(
            "SELECT state::text FROM joppa.board_cache WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0),
    )?;
    assert!(metadata.get("objects").is_none());
    assert!(metadata.get("requirements").is_none());
    assert!(metadata.to_string().len() < 20_000);
    assert!(conn.execute("UPDATE joppa.state_revisions SET value=jsonb_set(value,'{body}','\"changed contract\"'::jsonb) WHERE workspace=$1 AND section='requirements' AND key='r' AND revision=1", &[&ws]).await.is_err());
    // An unrelated invalid history row makes an accidental full history read fail.
    conn.execute("INSERT INTO joppa.state_revisions(workspace,section,key,revision,value) VALUES($1,'requirements','unrelated',1,'{}'::jsonb)", &[&ws]).await?;
    drop(conn);
    let current = store::read(
        &pool,
        &ws,
        "owner",
        &store::Scope::from_args(&json!({"req":"r"}))?,
    )
    .await?;
    assert_eq!(current["requirement"]["revision_count"], 2);
    assert_eq!(current["requirement"]["revisions"][0]["number"], 2);
    let old = store::read(
        &pool,
        &ws,
        "owner",
        &store::Scope::from_args(&json!({"req":"r","revision":1}))?,
    )
    .await?;
    assert_eq!(old["requirement"]["revisions"][0]["number"], 1);
    assert!(
        old["system_ids"]["ac"]["key"].is_string(),
        "the scoped read retains criterion copy identifiers"
    );
    pool.get().await?.execute("DELETE FROM joppa.state_revisions WHERE workspace=$1 AND section='requirements' AND key='unrelated'", &[&ws]).await?;
    let list = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            kind: Some("note"),
            parent: Some("*"),
            ..Default::default()
        },
    )
    .await?;
    assert!(!list.to_string().contains(&raw));
    let detail = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("note:raw"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(detail["document"]["data"]["body"], raw);
    // Updating the input must hydrate and preserve its earlier revision.
    store::execute_input(&pool,&ws,"agent","process-input",&json!({"op":"set_note_status","target":"note:raw","revision":1,"status":"processed","reason":"Processed source"})).await?;
    let old = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("note:raw"),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(old["document"]["data"]["body"], raw);
    let detail = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("note:raw"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(detail["document"]["data"]["body"], raw);
    assert_eq!(detail["document"]["data"]["status"], "processed");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn archived_groups_survive_current_state_reads_and_keep_their_history() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "requires a test database");
    store::migrate(&pool).await?;
    let ws = format!("archive-{}", uuid::Uuid::new_v4().simple());
    for (i, command) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{i}"), &command).await?;
    }
    let create = json!({"op":"domain","id":"unused","title":"Unused","owner":"owner"});
    let created = store::execute_remote_input(&pool, &ws, "owner", "create", &create).await?;
    let system_id = created["system_ids"]["domain"]["unused"].clone();
    assert!(system_id.is_string());
    store::execute_input(
        &pool,
        &ws,
        "agent",
        "context",
        &json!({"op":"note","target":"domain:unused","id":"context","text":"Retain this comment"}),
    )
    .await?;
    store::execute_input(&pool, &ws, "agent", "source", &json!({"op":"save_object","kind":"note","id":"source","revision":0,"parent":null,"data":{"title":"Source","owner":"owner","body":"Original source"},"links":[{"target":"domain:unused","relation":"related"}],"reason":"Link the source"})).await?;
    let scope = store::ObjectScope {
        address: Some("domain:unused"),
        ..Default::default()
    };
    let original = store::read_objects(&pool, &ws, "owner", &scope).await?;
    let archive = json!({"op":"archive_object","target":"domain:unused","revision":1,"reason":"Archive the group"});
    let archived = store::execute_remote_input(&pool, &ws, "owner", "archive", &archive).await?;
    let retry = store::execute_remote_input(&pool, &ws, "owner", "archive", &archive).await?;
    assert_eq!(retry["position"], archived["position"]);
    assert_eq!(retry["replayed"], true);
    let edit = json!({"op":"edit_domain","id":"unused","title":"Hidden edit","owner":"owner","reason":"Edit the group"});
    assert!(
        store::execute_input(&pool, &ws, "owner", "hidden-edit", &edit)
            .await
            .is_err()
    );
    store::execute_input(
        &pool,
        &ws,
        "agent",
        "unrelated",
        &json!({"op":"note","target":"domain:d","id":"unrelated","text":"Unrelated comment"}),
    )
    .await?;
    let current = store::read_objects(&pool, &ws, "owner", &scope).await?;
    assert_eq!(current["document"]["present"], false);
    assert_eq!(current["current_revision"], 2);
    assert_eq!(current["annotations"], original["annotations"]);
    assert_eq!(current["incoming"], original["incoming"]);
    let list = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            kind: Some("domain"),
            parent: Some("*"),
            ..Default::default()
        },
    )
    .await?;
    assert!(!list.to_string().contains("domain:unused"));
    let old = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("domain:unused"),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(old["document"], original["document"]);
    store::execute_remote_input(&pool, &ws, "owner", "restore", &json!({"op":"restore_object","target":"domain:unused","revision":2,"reason":"Restore the group"})).await?;
    let current = store::read_objects(&pool, &ws, "owner", &scope).await?;
    assert_eq!(current["item_id"], system_id);
    assert_eq!(current["document"]["present"], true);
    assert_eq!(current["current_revision"], 3);
    assert_eq!(current["annotations"], original["annotations"]);
    assert_eq!(current["incoming"], original["incoming"]);
    store::execute_input(&pool, &ws, "owner", "edit", &json!({"op":"edit_domain","id":"unused","title":"Changed","owner":"owner","reason":"Edit the restored group"})).await?;
    let current = store::read_objects(&pool, &ws, "owner", &scope).await?;
    assert_eq!(current["document"]["data"]["title"], "Changed");
    assert_eq!(current["current_revision"], 4);
    Ok(())
}
