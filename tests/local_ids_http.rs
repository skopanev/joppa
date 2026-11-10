//! A generated local ID is never taken, over a real server. The server of each test runs with
//! JOPPA_TEST_LOCAL_IDS in its own env only: each generation takes the next id of the list. The
//! Given objects use explicit ids, so they take no id of the list.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{msg, node, refused, schema};
use serde_json::{Value, json};

/// A server with the generated ids `ids`, for the record world with requirement:r.
async fn hooked(ids: &[&str]) -> Result<World> {
    record_world::world_env(&[], &[("JOPPA_TEST_LOCAL_IDS", ids.join(","))]).await
}

/// A save_object that creates a criterion under requirement:r, with the explicit `id` or none.
fn criterion(id: Option<&str>, title: &str) -> Value {
    let mut command = json!({"op":"save_object","kind":"ac","revision":0,
        "parent":"requirement:r","data":{"title":title,"owner":"owner"},"reason":"Create"});
    if let Some(id) = id {
        command["id"] = json!(id);
    }
    command
}

async fn ok(w: &World, command: Value) -> Result<Value> {
    w.command(&w.bot, command).await?.map_err(msg)
}

/// The address of a saved object in a reply.
fn address(reply: &Value) -> &str {
    reply["object"]["address"].as_str().unwrap_or("")
}

/// The title, the revision and the status tone of `address` in joppa_objects.
async fn kept(w: &World, address: &str) -> Result<(Value, Value, Value)> {
    let read = w.object(address).await?;
    Ok((
        read["data"]["title"].clone(),
        read["revision"].clone(),
        read["status"]["tone"].clone(),
    ))
}

/// The count of the objects of W.
async fn objects(w: &World) -> Result<i64> {
    let sql = "SELECT count(*) FROM joppa.objects WHERE workspace=$1";
    Ok(w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0))
}

// ac:gw4b0wmx
// test:0o020g8o
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_taken_generated_id_is_generated_again() -> Result<()> {
    let w = hooked(&["aaaaaaaa", "bbbbbbbb", "dddddddd", "aaaaaaaa"]).await?;
    let task = node("Tasks", "task", &[], json!({}));
    schema(&w, json!({"put_types":{"task":task}}))
        .await?
        .map_err(msg)?;
    ok(&w, criterion(Some("aaaaaaaa"), "Present")).await?;
    ok(&w, criterion(Some("bbbbbbbb"), "Archived")).await?;
    let archive =
        json!({"op":"archive_object","target":"ac:bbbbbbbb","revision":1,"reason":"Out of scope"});
    ok(&w, archive).await?;
    let before = [
        kept(&w, "ac:aaaaaaaa").await?,
        kept(&w, "ac:bbbbbbbb").await?,
    ];

    let created = ok(&w, criterion(None, "New")).await?;
    ensure!(address(&created) == "ac:dddddddd", "criterion: {created}");
    let task = json!({"op":"save_object","kind":"task","revision":0,"parent":null,
        "data":{"title":"Task","owner":"owner"},"reason":"Create"});
    let task = ok(&w, task).await?;
    ensure!(address(&task) == "task:aaaaaaaa", "task: {task}");

    ensure!(w.object("ac:dddddddd").await?["data"]["title"] == "New");
    ensure!(w.object("task:aaaaaaaa").await?["data"]["title"] == "Task");
    let after = [
        kept(&w, "ac:aaaaaaaa").await?,
        kept(&w, "ac:bbbbbbbb").await?,
    ];
    ensure!(after == before, "{after:?}, before {before:?}");
    Ok(())
}

// ac:xlujhhqg
// test:ao71avij
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_never_gives_one_generated_address_twice() -> Result<()> {
    let w = hooked(&["eeeeeeee", "eeeeeeee", "ffffffff"]).await?;
    let operations = json!([{"command":criterion(None, "First")},
        {"command":criterion(None, "Second")}]);
    let batch = json!({"request_id":"local-ids-1","operations":operations});
    let reply = w.mcp(&w.bot, "joppa_batch", batch).await?.map_err(msg)?;
    let results = [&reply["results"][0], &reply["results"][1]];
    let found = results.map(address);
    ensure!(found == ["ac:eeeeeeee", "ac:ffffffff"], "{reply}");
    ensure!(w.object("ac:eeeeeeee").await?["data"]["title"] == "First");
    ensure!(w.object("ac:ffffffff").await?["data"]["title"] == "Second");
    Ok(())
}

// ac:235xh1xg
// test:91c0h08m
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_explicit_id_and_an_exhausted_retry_are_refused() -> Result<()> {
    let w = hooked(&["aaaaaaaa"; 16]).await?;
    ok(&w, criterion(Some("aaaaaaaa"), "Present")).await?;
    let (position, count) = (w.position().await?, objects(&w).await?);

    let explicit = w
        .command(&w.bot, criterion(Some("aaaaaaaa"), "Again"))
        .await?;
    refused(explicit, "stale object revision; read the object again")?;
    let generated = w.command(&w.bot, criterion(None, "New")).await?;
    refused(
        generated,
        "no free local ID after 16 attempts; retry the write",
    )?;
    ensure!(w.position().await? == position, "the journal grows");
    ensure!(objects(&w).await? == count, "W gets a new object");
    Ok(())
}
