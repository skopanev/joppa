//! The dimension tones that one write stores equal the full rebuild of the status projection,
//! after each kind of write: a save, a dimension change, a new node, a record, a move, a link,
//! a note and an archive.
#[path = "common/dims_tree.rs"]
mod dims_tree;
use anyhow::{Result, ensure};
use dims_tree::{Pool, node, record, stored, tree};
use joppa::store;
use serde_json::{Value, json};

async fn write(pool: &Pool, ws: &str, command: Value) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, "owner", &id, &command).await?;
    Ok(())
}

/// A save of `address` with `change` merged into its command: data, parent or relations.
async fn save(pool: &Pool, ws: &str, address: &str, change: Value) -> Result<()> {
    let scope = store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    let object = store::read_objects(pool, ws, "owner", &scope).await?;
    let (kind, id) = address.split_once(':').unwrap();
    let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],
        "parent":object["document"]["parent"],"data":{},"reason":"Change"});
    for (key, value) in change.as_object().unwrap() {
        command[key] = value.clone();
    }
    write(pool, ws, command).await
}

async fn archive(pool: &Pool, ws: &str, address: &str) -> Result<()> {
    let scope = store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    let object = store::read_objects(pool, ws, "owner", &scope).await?;
    let command = json!({"op":"archive_object","target":address,
        "revision":object["current_revision"],"reason":"Not needed"});
    write(pool, ws, command).await
}

/// The stored values equal the values of a full rebuild.
async fn same_as_rebuild(pool: &Pool, ws: &str, step: &str) -> Result<()> {
    let written = stored(pool, ws).await?;
    store::catch_up_statuses(pool, true).await?;
    let rebuilt = stored(pool, ws).await?;
    let differ: Vec<_> = written
        .iter()
        .zip(&rebuilt)
        .filter(|(a, b)| a != b)
        .take(3)
        .collect();
    ensure!(
        written.len() == rebuilt.len() && differ.is_empty(),
        "{step}: {differ:?}"
    );
    ensure!(rebuilt.iter().any(|r| r.2.is_some()), "{step}: no tones");
    Ok(())
}

// ac:jindi00h
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_dimension_tones_of_a_write_equal_the_full_rebuild() -> Result<()> {
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
    let (ws, tests) = tree(&pool, 1, 4).await?;
    record(&pool, &ws, &tests, 2).await?;
    same_as_rebuild(&pool, &ws, "seed").await?;
    let body = json!({"data":{"body":"The new rule"}});
    save(&pool, &ws, "ac:d0c1r2a1", body).await?;
    same_as_rebuild(&pool, &ws, "save").await?;
    // ac:d0c0r0a0 has env prod, and its test node inherits the value.
    save(&pool, &ws, "ac:d0c0r0a0", json!({"data":{"env":"dev"}})).await?;
    same_as_rebuild(&pool, &ws, "dimension change").await?;
    let new = node("test", "d0c1r2a1x", Some("ac:d0c1r2a1".into()));
    write(&pool, &ws, new).await?;
    same_as_rebuild(&pool, &ws, "new node").await?;
    let one =
        json!([{"test":tests[5],"status":"fail","env":"stage"},{"test":tests[6],"status":"fail"}]);
    let args = json!({"commit":"b000001","results":one});
    store::record_results(&pool, &ws, "owner", &args).await?;
    same_as_rebuild(&pool, &ws, "record").await?;
    let moved = json!({"parent":"ac:d0c0r0a3"});
    save(&pool, &ws, "test:d0c1r0a2t", moved).await?;
    same_as_rebuild(&pool, &ws, "move").await?;
    let link = json!({"relations":{"depends_on":{"edges":[{"to":"ac:d0c2r1a0"}]}}});
    save(&pool, &ws, "ac:d0c3r1a1", link).await?;
    same_as_rebuild(&pool, &ws, "link").await?;
    let note = json!({"op":"note","target":"ac:d0c3r1a1","id":"n1","text":"Seen"});
    write(&pool, &ws, note).await?;
    same_as_rebuild(&pool, &ws, "note").await?;
    archive(&pool, &ws, "test:d0c4r5a3t").await?;
    same_as_rebuild(&pool, &ws, "archive").await?;
    Ok(())
}
