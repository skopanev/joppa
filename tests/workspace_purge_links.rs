//! A deletion names the workspaces that link to the deleted workspace and removes those
//! links: remote links and child workspace bindings. Each change is an event of the
//! surviving workspace with a new revision. Old dangling links leave at the migration.
#[path = "common/purge_world.rs"]
mod purge_world;
use anyhow::{Context, Result, ensure};
use joppa::store;
use purge_world::{pool, workspace, write};
use serde_json::{Value, json};

async fn read(pool: &deadpool_postgres::Pool, ws: &str, address: &str) -> Result<Value> {
    store::read_objects(
        pool,
        ws,
        "owner",
        &store::ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await
}

/// P links to C from note:n and binds domain:backend to C. note:n also links to K.
async fn linked() -> Result<(deadpool_postgres::Pool, String, String, String)> {
    let pool = pool().await?;
    let (p, _) = workspace(&pool, "product", "p").await?;
    let (c, _) = workspace(&pool, "component", "c").await?;
    let (k, _) = workspace(&pool, "kept", "k").await?;
    for ws in [&c, &k] {
        write(
            &pool,
            ws,
            "owner",
            json!({"op":"save_object","kind":"domain","id":"api","revision":0,"parent":null,
            "data":{"title":"API contract","owner":"owner"},"reason":"Fixture"}),
        )
        .await?;
    }
    write(
        &pool,
        &p,
        "owner",
        json!({"op":"save_object","kind":"domain","id":"backend","revision":0,"parent":null,
        "data":{"title":"Backend","owner":"owner","workspace":c},"reason":"Fixture"}),
    )
    .await?;
    let note = read(&pool, &p, "note:n").await?;
    write(&pool, &p, "owner", json!({"op":"set_remote_links","target":"note:n","revision":note["remote_links_revision"],"reason":"Uses the API",
        "links":[{"workspace":c,"target":"domain:api","relation":"related","revision":1,"blocking":false},
                 {"workspace":c,"target":"domain:api","relation":"depends_on","revision":1,"blocking":true},
                 {"workspace":k,"target":"domain:api","relation":"related","revision":1,"blocking":false}]})).await?;
    Ok((pool, p, c, k))
}

/// The links of P name only K, and domain:backend has a new revision without the binding.
async fn released(pool: &deadpool_postgres::Pool, p: &str, k: &str) -> Result<()> {
    let note = read(pool, p, "note:n").await?;
    let links = note["remote_links"].as_array().context("remote links")?;
    ensure!(links.len() == 1 && links[0]["workspace"] == k, "{note}");
    ensure!(note["remote_links_revision"] == 2, "{note}");
    let node = read(pool, p, "domain:backend").await?;
    ensure!(
        node["document"]["data"].get("workspace").is_none(),
        "{node}"
    );
    ensure!(
        node["document"]["revision"] == 2 && node["document"]["actor"] == "joppa-deletion",
        "{node}"
    );
    ensure!(node.get("child_workspace").is_none(), "{node}");
    let events: i64 = pool.get().await?.query_one("SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='remove_workspace_references' AND actor='joppa-deletion'", &[&p]).await?.get(0);
    ensure!(events == 1, "{events} removal events in P");
    // A replay of every journal gives the same state, the removal event included.
    store::check_journal(pool).await
}

// owner-manages-workspace-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn owner_manages_workspace_ac4_a_deletion_names_and_removes_the_links_of_other_workspaces()
-> Result<()> {
    let (pool, p, c, k) = linked().await?;
    // Before the deletion, the owner reads the workspaces that link to C. A member cannot.
    let listed = store::workspace_references(&pool, &c, "owner").await?;
    let expected = json!([{"id":p,"title":"product","objects":["domain:backend","note:n"]}]);
    ensure!(listed["workspaces"] == expected, "{listed}");
    ensure!(
        store::workspace_references(&pool, &c, "alex")
            .await
            .is_err(),
        "a member read the references"
    );
    let deleted = write(
        &pool,
        &c,
        "owner",
        json!({"op":"delete_workspace","reason":"Owner order"}),
    )
    .await?;
    ensure!(deleted["removed_references"] == expected, "{deleted}");
    released(&pool, &p, &k).await?;
    // K keeps its object, and the link of P to K still reads as accessible.
    let note = read(&pool, &p, "note:n").await?;
    ensure!(
        note["remote_links"][0]["status"]["access"] == true,
        "{note}"
    );
    Ok(())
}

// owner-manages-workspace-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn links_to_a_workspace_purged_before_leave_at_the_migration() -> Result<()> {
    let (pool, p, c, k) = linked().await?;
    // An earlier purge removed C and left the links of P.
    pool.get()
        .await?
        .execute("SELECT joppa.purge_workspace($1)", &[&c])
        .await?;
    ensure!(
        read(&pool, &p, "note:n").await?["remote_links"]
            .as_array()
            .is_some_and(|l| l.len() == 3),
        "the links left early"
    );
    store::migrate(&pool).await?;
    released(&pool, &p, &k).await
}
