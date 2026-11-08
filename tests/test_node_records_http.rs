//! requirement:test-nodes over a real server: a record names a test node by its address, by its
//! public item ID or by a title that exactly one present test node has. Each result is stored
//! under the address of its node, so a renamed node keeps its results.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const REFUND: &str = "A refund over the limit is refused";
const RENAMED: &str = "A refund above the limit is refused";

async fn command(w: &record_world::World, token: &str, command: Value) -> Result<()> {
    w.command(token, command.clone())
        .await?
        .map_err(|e| anyhow::anyhow!("{command}: {e}"))?;
    Ok(())
}

fn node(id: &str, parent: &str, title: &str) -> Value {
    json!({"op":"save_object","kind":"test","id":id,"revision":0,"parent":parent,
        "data":{"title":title,"owner":"owner"},"reason":"Bind"})
}

async fn tone(w: &record_world::World, address: &str) -> Result<String> {
    Ok(w.object(address).await?["status"]["tone"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// The stored address of each result of the workspace, in record order.
async fn stored(w: &record_world::World) -> Result<Vec<String>> {
    let sql = "SELECT t.address FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 ORDER BY r.id,t.address";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

// ac:76a39hpq
// test:8twunvc4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_record_by_address_or_item_id_binds_the_node_and_a_rename_keeps_the_results() -> Result<()>
{
    let w = record_world::world(&[]).await?;
    command(&w, &w.bot, node("t", "ac:a", REFUND)).await?;
    let first = w
        .recorded(&"a".repeat(40), json!([{"test":"test:t","status":"pass"}]))
        .await?;
    ensure!(
        first["unbound"] == json!([]) && first["tests"] == 1,
        "{first}"
    );
    let scope = joppa::store::ObjectScope {
        address: Some("test:t"),
        ..Default::default()
    };
    let read = joppa::store::read_objects(&w.pool, &w.ws, "bot", &scope).await?;
    let item_id = read["item_id"].as_str().unwrap_or_default().to_owned();
    ensure!(!item_id.is_empty() && !item_id.contains(':'), "{item_id}");
    let second = w
        .recorded(&"b".repeat(40), json!([{"test":item_id,"status":"pass"}]))
        .await?;
    ensure!(
        second["unbound"] == json!([]) && second["tests"] == 1,
        "{second}"
    );
    ensure!(tone(&w, "test:t").await? == "green" && tone(&w, "ac:a").await? == "green");
    // The owner renames T. Both records stay bound to it, and nothing resets.
    let revision = w.object("test:t").await?["revision"].clone();
    command(
        &w,
        &w.owner,
        json!({"op":"save_object","kind":"test","id":"t","revision":revision,
        "parent":"ac:a","data":{"title":RENAMED,"owner":"owner"},"reason":"Clearer words"}),
    )
    .await?;
    ensure!(tone(&w, "test:t").await? == "green", "T after the rename");
    ensure!(tone(&w, "ac:a").await? == "green", "A after the rename");
    let status = w.a_status(&w.bot).await?;
    let row = record_world::test(&status, RENAMED);
    ensure!(
        status["status"] == "green" && row["status"] == "pass",
        "{status}"
    );
    // A record by the earlier title still binds T.
    let old = w
        .recorded(&"c".repeat(40), json!([{"test":REFUND,"status":"pass"}]))
        .await?;
    ensure!(old["unbound"] == json!([]), "{old}");
    ensure!(
        tone(&w, "test:t").await? == "green",
        "T after the old title"
    );
    // Each stored result names the address of T.
    let rows = stored(&w).await?;
    ensure!(rows == ["test:t", "test:t", "test:t"], "{rows:?}");
    Ok(())
}

// ac:sekr3jz9
// test:0xieduhi
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_title_binds_every_node_with_it_an_id_binds_one_and_an_unknown_title_is_unbound()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    command(
        &w,
        &w.bot,
        json!({"op":"save_object","kind":"ac","id":"b","revision":0,
        "parent":"requirement:r","data":{"title":"B","owner":"owner"},"reason":"Create"}),
    )
    .await?;
    for (id, parent, title) in [
        ("u", "ac:a", "Login works"),
        ("out-a", "ac:a", "Logout works"),
        ("out-b", "ac:b", "Logout works"),
    ] {
        command(&w, &w.bot, node(id, parent, title)).await?;
    }
    // A title binds every present test node with it: one row for each node.
    let shared = w
        .recorded(
            &"a".repeat(40),
            json!([{"test":"Login works","status":"pass"},
            {"test":"Logout works","status":"pass"}]),
        )
        .await?;
    ensure!(shared["unbound"] == json!([]), "{shared}");
    let rows = stored(&w).await?;
    ensure!(rows == ["test:out-a", "test:out-b", "test:u"], "{rows:?}");
    for address in ["test:u", "test:out-a", "test:out-b", "ac:a", "ac:b"] {
        ensure!(
            tone(&w, address).await? == "green",
            "{address} after the shared title"
        );
    }
    // An address binds that one node only.
    let one = w
        .recorded(
            &"b".repeat(40),
            json!([{"test":"test:out-b","status":"fail"}]),
        )
        .await?;
    ensure!(one["unbound"] == json!([]), "{one}");
    let rows = stored(&w).await?;
    ensure!(rows[3..] == ["test:out-b"], "{rows:?}");
    ensure!(tone(&w, "test:out-b").await? == "red" && tone(&w, "ac:b").await? == "red");
    ensure!(tone(&w, "test:out-a").await? == "green" && tone(&w, "ac:a").await? == "green");
    // A title with no test node is unbound, and the call does not fail.
    let signup = w
        .recorded(
            &"c".repeat(40),
            json!([{"test":"Signup works","status":"pass"}]),
        )
        .await?;
    ensure!(signup["unbound"] == json!(["Signup works"]), "{signup}");
    let rows = stored(&w).await?;
    ensure!(rows[4..] == ["Signup works"], "{rows:?}");
    Ok(())
}
