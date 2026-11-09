//! The fixture of the source reset tests on a real server. Type src has source: true, type doc
//! has no flag, and type ac has the reference field sources (multiple, target src). Criterion
//! AA names source S in sources, BB has a related link to S, HH has a depends_on link to S with
//! blocking: false, and S has a related link to KK. CC has a related link to document D of type
//! doc, and EE names nothing. Each criterion has one test node, and a record passes every test.
#![allow(dead_code)]
use crate::record_world::{self, World};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// The criteria that name source S: A, B, H and K of the criterion.
pub const NAMED: [&str; 4] = ["ac:aa", "ac:bb", "ac:hh", "ac:kk"];
/// The criteria that do not name S: C and E of the criterion.
pub const OTHER: [&str; 2] = ["ac:cc", "ac:ee"];

fn msg(e: String) -> anyhow::Error {
    anyhow::Error::msg(e)
}

/// One command of the owner that must succeed.
pub async fn owner(w: &World, command: Value) -> Result<Value> {
    let text = command.to_string();
    w.command(&w.owner, command)
        .await?
        .map_err(|e| anyhow::anyhow!("{text}: {e}"))
}

/// A new criterion under requirement:r with `data` and `links`, and its test node.
async fn criterion(w: &World, id: &str, data: Value, links: Value) -> Result<()> {
    let mut d = json!({"title":id.to_uppercase(),"owner":"owner"});
    for (k, v) in data.as_object().into_iter().flatten() {
        d[k] = v.clone();
    }
    owner(
        w,
        json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:r",
        "data":d,"links":links,"reason":"Create the criterion"}),
    )
    .await?;
    owner(
        w,
        json!({"op":"save_object","kind":"test","revision":0,"parent":format!("ac:{id}"),
        "data":{"title":test(&format!("ac:{id}")),"owner":"owner"},"reason":"Bind the test"}),
    )
    .await?;
    Ok(())
}

/// The test name of a criterion.
pub fn test(address: &str) -> String {
    format!("tests/source.rs > {}", address.trim_start_matches("ac:"))
}

/// The record `n` that passes the test of each criterion.
pub async fn record(w: &World, n: u32) -> Result<Value> {
    let results: Vec<Value> = NAMED
        .iter()
        .chain(&OTHER)
        .map(|a| json!({"test":test(a),"status":"pass"}))
        .collect();
    w.recorded(&format!("5a5a5a{n:x}"), json!(results)).await
}

/// Build the fixture. Each criterion is green at the end.
pub async fn world() -> Result<World> {
    let w = record_world::world(&[]).await?;
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({}))
        .await?
        .map_err(msg)?;
    let root = |title: &str| json!({"title":title,"role":"document","status":"none","root":true,"parents":[]});
    let mut src = root("Sources");
    src["source"] = json!(true);
    let ac = json!({"title":"Criteria","role":"ac","status":"children","parents":["requirement"],
        "references":{"sources":{"target":"src","title":"Sources","multiple":true}}});
    owner(
        &w,
        json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "put_types":{"src":src,"doc":root("Documents"),"ac":ac},"reason":"Add the source types"}),
    )
    .await?;
    criterion(&w, "kk", json!({}), json!([])).await?;
    owner(
        &w,
        json!({"op":"save_object","kind":"src","id":"s","revision":0,"parent":null,
        "data":{"title":"S","owner":"owner","body":"The source text"},
        "links":[{"target":"ac:kk","relation":"related"}],"reason":"Create the source"}),
    )
    .await?;
    owner(&w, json!({"op":"save_object","kind":"doc","id":"d","revision":0,"parent":null,
        "data":{"title":"D","owner":"owner","body":"The document text"},"reason":"Create the document"}))
    .await?;
    criterion(&w, "aa", json!({"sources":["src:s"]}), json!([])).await?;
    let related = |target: &str| json!([{"target":target,"relation":"related"}]);
    criterion(&w, "bb", json!({}), related("src:s")).await?;
    let depends = json!([{"target":"src:s","relation":"depends_on","blocking":false}]);
    criterion(&w, "hh", json!({}), depends).await?;
    criterion(&w, "cc", json!({}), related("doc:d")).await?;
    criterion(&w, "ee", json!({}), json!([])).await?;
    record(&w, 0).await?;
    expect(&w, "setup", &[]).await?;
    Ok(w)
}

/// Save `patch` into the data of `address` as the owner. The other fields, the parent and the
/// links stay.
pub async fn save(w: &World, address: &str, patch: Value) -> Result<Value> {
    let current = w.object(address).await?;
    let (kind, id) = address.split_once(':').context("address")?;
    owner(
        w,
        json!({"op":"save_object","kind":kind,"id":id,"revision":current["revision"],
        "data":patch,"reason":"Change the source"}),
    )
    .await
}

/// The owner attaches a new file to S. Gives the file ID.
pub async fn attach(w: &World) -> Result<String> {
    let content = format!("Source file {}", uuid::Uuid::new_v4());
    let args = json!({"target":"src:s","filename":"source.txt","content_type":"text/plain","content":content});
    let reply = w.mcp(&w.owner, "joppa_attach", args).await?.map_err(msg)?;
    Ok(reply["file"]["id"].as_str().context("file id")?.to_owned())
}

/// The owner detaches the file `id` from S.
pub async fn detach(w: &World, id: &str) -> Result<Value> {
    owner(
        w,
        json!({"op":"detach","target":"src:s","id":id,"reason":"Drop the file"}),
    )
    .await
}

/// The last journal position of the workspace.
pub async fn position(w: &World) -> Result<i64> {
    let sql = "SELECT max(position) FROM joppa.events WHERE workspace=$1";
    Ok(w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0))
}

/// The status of each criterion from store::test_status and the joppa_objects tone: the
/// criteria in `grey` are grey, every other criterion is green.
pub async fn expect(w: &World, step: &str, grey: &[&str]) -> Result<()> {
    for address in NAMED.iter().chain(&OTHER) {
        let want = if grey.contains(address) {
            "grey"
        } else {
            "green"
        };
        let status = joppa::store::test_status(&w.pool, &w.ws, "owner", address).await?;
        let tone = w.object(address).await?["status"]["tone"].clone();
        ensure!(
            status["status"] == want && tone == want,
            "{step}: {address} must be {want}: test_status {status}, tone {tone}"
        );
    }
    Ok(())
}
