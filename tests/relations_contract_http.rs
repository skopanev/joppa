//! requirement:q438uw0s over a real server: a flow change resets no criterion, keeps the
//! contract and resets nothing on a source. A new workspace starts with the flow-neutral reset.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::{A, World};
use relations_world::{boundaries, change, declare, msg, save_as, saved, stored, tone};
use serde_json::{Value, json};

const TEST: &str = "tests/relations_contract_http.rs > a";

/// The tone and the test status of criterion `address` are both `want`.
async fn status(w: &World, address: &str, want: &str, step: &str) -> Result<()> {
    let status = joppa::store::test_status(&w.pool, &w.ws, "owner", address).await?;
    let tone = tone(w, address).await?;
    ensure!(
        status["status"] == want && tone == want,
        "{step}: {address} must be {want}: test_status {status}, tone {tone}"
    );
    Ok(())
}

/// The review flag of the first remote link of `address` in workspace `ws`, read by the owner.
async fn review(w: &World, ws: &str, address: &str) -> Result<Value> {
    let scope = joppa::store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    let read = joppa::store::read_objects(&w.pool, ws, "owner", &scope).await?;
    Ok(read["remote_links"][0]["status"]["review_required"].clone())
}

// ac:8kphub4v
// test:mkz86ej5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_flow_change_resets_no_criterion_and_keeps_the_contract() -> Result<()> {
    let w = relations_world::world(&[TEST]).await?;
    saved(&w, "doc:s", 0, json!({})).await?;
    w.recorded("5a5a5a1", json!([{"test":TEST,"status":"pass"}]))
        .await?;
    status(&w, A, "green", "setup").await?;
    // Workspace V pins criterion A with a remote link.
    let v = format!("consumer-{}", uuid::Uuid::new_v4().simple());
    let write = async |command: Value| {
        let request = uuid::Uuid::new_v4().to_string();
        joppa::store::execute_remote_input(&w.pool, &v, "owner", &request, &command).await
    };
    let create = json!({"op":"workspace","title":"Consumer"});
    joppa::store::execute_input(&w.pool, &v, "owner", "consumer-1", &create).await?;
    write(
        json!({"op":"save_object","kind":"domain","id":"use","revision":0,"parent":null,
        "data":{"title":"Use","owner":"owner"},"reason":"Fixture"}),
    )
    .await?;
    let pin = stored(&w, A).await?.0;
    let link =
        json!({"workspace":w.ws,"target":A,"relation":"depends_on","revision":pin,"blocking":true});
    write(
        json!({"op":"set_remote_links","target":"domain:use","revision":0,"links":[link],
        "reason":"Uses the criterion"}),
    )
    .await?;
    ensure!(
        review(&w, &v, "domain:use").await? == false,
        "the pin is current"
    );
    declare(&w, &[("swap", "Swap")]).await?;
    change(&w, A, json!({"relations":{"swap":{"title":"Next"}}})).await?;
    status(&w, A, "green", "a new swap key").await?;
    ensure!(
        review(&w, &v, "domain:use").await? == false,
        "after a new swap key"
    );
    change(
        &w,
        A,
        json!({"relations":{"swap":{"edges":[{"to":"doc:s"}]}}}),
    )
    .await?;
    status(&w, A, "green", "new swap edges").await?;
    ensure!(
        review(&w, &v, "domain:use").await? == false,
        "after new swap edges"
    );
    Ok(())
}

// ac:4xbl6uha
// test:049tu1s1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_flow_on_a_source_resets_nothing() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    save_as(&w, &w.owner, "src:s", 0, json!({}))
        .await?
        .map_err(msg)?;
    for (id, relation) in [("aa", "swap"), ("bb", "related")] {
        let address = format!("ac:{id}");
        let edge = json!({relation:{"edges":[{"to":"src:s"}]}});
        let extra = json!({"parent":"requirement:r","relations":edge});
        save_as(&w, &w.owner, &address, 0, extra)
            .await?
            .map_err(msg)?;
        let test = json!({"op":"save_object","kind":"test","revision":0,"parent":address,
            "data":{"title":format!("{TEST}{id}"),"owner":"owner"},"reason":"Bind the test"});
        w.command(&w.owner, test).await?.map_err(msg)?;
    }
    let pass = |id: &str| json!({"test":format!("{TEST}{id}"),"status":"pass"});
    w.recorded("5a5a5a2", json!([pass("aa"), pass("bb")]))
        .await?;
    status(&w, "ac:aa", "green", "setup").await?;
    status(&w, "ac:bb", "green", "setup").await?;
    let before = boundaries(&w).await?;
    save_as(
        &w,
        &w.owner,
        "src:s",
        1,
        json!({"body":"A new source text"}),
    )
    .await?
    .map_err(msg)?;
    let after = boundaries(&w).await?;
    let of = |list: &[(String, String)], key: &str| {
        list.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    ensure!(
        of(&before, "ac:aa") == of(&after, "ac:aa"),
        "A keeps its boundary: {after:?}"
    );
    let moved = of(&after, "ac:bb").is_some() && of(&before, "ac:bb") != of(&after, "ac:bb");
    ensure!(moved, "B gets a new boundary: {before:?} {after:?}");
    status(&w, "ac:aa", "green", "after the source save").await?;
    status(&w, "ac:bb", "blue", "after the source save").await
}

// ac:8cp0n5ec
// test:ql5scc4r
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_workspace_starts_with_the_flow_neutral_reset() -> Result<()> {
    let w = relations_world::world(&[TEST]).await?;
    let sql = "SELECT first_position FROM joppa.replay_epochs WHERE workspace=$1 AND rule='relations_reset'";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    let epoch: Vec<i64> = rows.iter().map(|r| r.get(0)).collect();
    ensure!(epoch == [1], "relations_reset of W: {epoch:?}");
    declare(&w, &[("swap", "Swap")]).await?;
    saved(&w, "doc:s", 0, json!({})).await?;
    w.recorded("5a5a5a3", json!([{"test":TEST,"status":"pass"}]))
        .await?;
    status(&w, A, "green", "setup").await?;
    change(
        &w,
        A,
        json!({"relations":{"swap":{"edges":[{"to":"doc:s"}]}}}),
    )
    .await?;
    status(&w, A, "green", "after the swap edge").await
}
