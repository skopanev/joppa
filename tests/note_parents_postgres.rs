//! requirement:note-has-no-parent: an inbox type and an inbox save with a parent are refused,
//! and the migration moves an older note to the root with a related link.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::store::{self, ObjectScope};
use serde_json::{Value, json};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn db() -> Result<(deadpool_postgres::Pool, String)> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok((pool, url))
}

fn save(kind: &str, id: &str, revision: u32, parent: Option<&str>, data: Value) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Note parent test"})
}

async fn read(pool: &deadpool_postgres::Pool, ws: &str, address: &str) -> Result<Value> {
    store::read_objects(
        pool,
        ws,
        "owner",
        &ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await
}

// note-parent-refused
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_inbox_parent_rule_and_an_inbox_save_with_a_parent_are_refused() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = format!("notes-{}", uuid::Uuid::new_v4().simple());
    let write = |command: Value| {
        let (pool, ws) = (pool.clone(), ws.clone());
        async move {
            store::execute_input(
                &pool,
                &ws,
                "owner",
                &uuid::Uuid::new_v4().to_string(),
                &command,
            )
            .await
        }
    };
    write(json!({"op":"workspace","title":"Notes"})).await?;
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    write(save(
        "domain",
        "d",
        0,
        None,
        json!({"title":"Billing","owner":"owner"}),
    ))
    .await?;
    write(save(
        "capability",
        "c",
        0,
        Some("domain:d"),
        json!({"title":"Pay","owner":"owner"}),
    ))
    .await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    ensure!(
        tree["schema"]["types"]["note"]["parents"] == json!([]),
        "{tree}"
    );
    let mut note = tree["schema"]["types"]["note"].clone();
    note["parents"] = json!(["*"]);
    let schema = write(json!({"op":"set_tree_schema","revision":tree["current_revision"],"put_types":{"note":note},"reason":"Give notes a parent"})).await;
    ensure!(schema.is_err(), "the inbox parent rule was saved");
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    let refused = world::mcp(&server.root, "bot", "joppa_command", json!({"workspace":ws,"request_id":"note-under-c",
        "command":save("note", "n", 0, Some("capability:c"), json!({"title":"Input","owner":"owner","body":"Source"}))})).await;
    let error = refused.err().map(|e| e.to_string()).unwrap_or_default();
    ensure!(
        error.contains("links") && error.contains("related"),
        "the error does not name links: {error}"
    );
    let after = store::tree_schema(&pool, &ws, "owner", None).await?;
    ensure!(
        after["current_revision"] == tree["current_revision"],
        "the schema changed"
    );
    ensure!(
        read(&pool, &ws, "note:n").await.is_err(),
        "the note was saved"
    );
    Ok(())
}

// note-parent-migration
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_migration_moves_a_note_to_the_root_with_a_related_link() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, _) = db().await?;
    let ws = format!("before-{}", uuid::Uuid::new_v4().simple());
    // A journal from before migrate_note_parents: three revisions of note n under capability c.
    let mut events = vec![json!({"op":"workspace","title":"Before notes"})];
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_task_claims",
        "migrate_persistent_claims",
        "migrate_consumers",
        "migrate_consumer_drops",
        "migrate_test_rules",
    ] {
        events.push(json!({ "op": op }));
    }
    events.push(save(
        "domain",
        "d",
        0,
        None,
        json!({"title":"Billing","owner":"owner"}),
    ));
    events.push(save(
        "capability",
        "c",
        0,
        Some("domain:d"),
        json!({"title":"Pay","owner":"owner"}),
    ));
    for (n, body) in ["One", "Two", "Three"].iter().enumerate() {
        events.push(save(
            "note",
            "n",
            n as u32,
            Some("capability:c"),
            json!({"title":"Input","owner":"owner","body":body}),
        ));
    }
    let conn = pool.get().await?;
    conn.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&ws])
        .await?;
    for (i, command) in events.iter().enumerate() {
        conn.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,'owner',$3,$4::text::jsonb)",
            &[&ws, &(i as i64 + 1), &uuid::Uuid::new_v4().to_string(), &command.to_string()]).await?;
    }
    drop(conn);
    // The server starts twice.
    store::migrate(&pool).await?;
    let position = |pool: deadpool_postgres::Pool, ws: String| async move {
        Ok::<i64, anyhow::Error>(
            pool.get()
                .await?
                .query_one(
                    "SELECT max(position) FROM joppa.events WHERE workspace=$1",
                    &[&ws],
                )
                .await?
                .get(0),
        )
    };
    let first = position(pool.clone(), ws.clone()).await?;
    store::migrate(&pool).await?;
    let second = position(pool.clone(), ws.clone()).await?;
    let n = read(&pool, &ws, "note:n").await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    ensure!(n["document"]["parent"].is_null(), "{n}");
    ensure!(
        n["links"].as_array().is_some_and(|l| l
            .iter()
            .any(|l| l["link"]["target"] == "capability:c" && l["link"]["relation"] == "related")),
        "{n}"
    );
    ensure!(
        n["current_revision"] == 4 && n["document"]["data"]["body"] == "Three",
        "{n}"
    );
    for r in 1..=3u32 {
        let old = store::read_objects(
            &pool,
            &ws,
            "owner",
            &ObjectScope {
                address: Some("note:n"),
                revision: Some(r),
                ..Default::default()
            },
        )
        .await?;
        ensure!(
            old["document"]["parent"] == "capability:c",
            "revision {r}: {old}"
        );
    }
    ensure!(
        tree["schema"]["types"]["note"]["parents"] == json!([]),
        "{tree}"
    );
    ensure!(
        first == second,
        "the second start wrote {} events",
        second - first
    );
    Ok(())
}
