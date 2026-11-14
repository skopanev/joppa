//! A write that copies the last revision of an object gives the new revision its own alias.
#[path = "common/dims_tree.rs"]
mod dims_tree;
use anyhow::{Result, ensure};
use dims_tree::{Pool, tree};
use joppa::{actor_alias, store};
use serde_json::{Value, json};

/// Run `command` as member owner with `alias`.
async fn write(pool: &Pool, ws: &str, alias: &str, command: Value) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let work = store::execute_remote_input(pool, ws, "owner", &id, &command);
    actor_alias::scope(Some(alias.to_owned()), work).await?;
    Ok(())
}

/// The alias of each stored revision of `address`, and of the journal event of each.
async fn aliases(pool: &Pool, ws: &str, address: &str) -> Result<Vec<(String, String)>> {
    let sql = "SELECT r.document->>'alias',e.alias FROM joppa.object_revisions r JOIN joppa.events e ON e.workspace=r.workspace AND e.position=(r.document->>'position')::bigint WHERE r.workspace=$1 AND r.address=$2 ORDER BY r.revision";
    let rows = pool.get().await?.query(sql, &[&ws, &address]).await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

async fn revision(pool: &Pool, ws: &str, address: &str) -> Result<Value> {
    let scope = store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    Ok(store::read_objects(pool, ws, "owner", &scope).await?["current_revision"].clone())
}

// ac:5wnpx78i
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archive_and_a_restore_keep_the_alias_of_their_own_write() -> Result<()> {
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
    let (ws, _) = tree(&pool, 1, 1).await?;
    let t = "domain:dx";
    let save = json!({"op":"save_object","kind":"domain","id":"dx","revision":0,"parent":null,
        "data":{"title":"Domain X","owner":"owner","body":"The rule"},"reason":"Create"});
    write(&pool, &ws, "a1", save).await?;
    let archive = json!({"op":"archive_object","target":t,"revision":revision(&pool, &ws, t).await?,"reason":"Not needed"});
    write(&pool, &ws, "a2", archive).await?;
    let restore = json!({"op":"restore_object","target":t,"revision":revision(&pool, &ws, t).await?,"reason":"Needed again"});
    write(&pool, &ws, "a3", restore).await?;
    let found = aliases(&pool, &ws, t).await?;
    let wanted: Vec<(String, String)> = ["a1", "a2", "a3"]
        .iter()
        .map(|a| (a.to_string(), a.to_string()))
        .collect();
    ensure!(found == wanted, "{found:?}");
    // A replay of the journal gives the same rows.
    store::check_journal(&pool).await?;
    Ok(())
}
