//! requirement:q438uw0s over a real server: remote links and the workspace deletion keep
//! working beside relations.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{edges, saved, to};
use serde_json::{Value, json};

/// One write of the owner in workspace `ws` through the store.
async fn write(w: &World, ws: &str, command: Value) -> Result<Value> {
    let request = uuid::Uuid::new_v4().to_string();
    joppa::store::execute_remote_input(&w.pool, ws, "owner", &request, &command).await
}

/// The read of `address` in workspace `ws` by the owner, in the HTTP shape.
async fn read(w: &World, ws: &str, address: &str) -> Result<Value> {
    let scope = joppa::store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    joppa::store::read_objects(&w.pool, ws, "owner", &scope).await
}

/// A workspace of the owner with the node domain:api.
async fn workspace(w: &World, prefix: &str) -> Result<String> {
    let ws = format!("{prefix}-{}", uuid::Uuid::new_v4().simple());
    let create = json!({"op":"workspace","title":prefix});
    joppa::store::execute_input(&w.pool, &ws, "owner", &format!("{prefix}-1"), &create).await?;
    write(
        w,
        &ws,
        json!({"op":"save_object","kind":"domain","id":"api","revision":0,
        "parent":null,"data":{"title":"API","owner":"owner"},"reason":"Fixture"}),
    )
    .await?;
    Ok(ws)
}

fn remote(ws: &str, relation: &str, blocking: bool) -> Value {
    json!({"workspace":ws,"target":"domain:api","relation":relation,"revision":1,"blocking":blocking})
}

// ac:yk7ypz6b
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn remote_links_and_the_workspace_deletion_keep_working() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    let p = w.ws.clone();
    let d = workspace(&w, "deleted").await?;
    let k = workspace(&w, "kept").await?;
    // In P, doc:n binds D, and doc:u has a related edge to doc:n and remote links.
    relations_world::save_as(&w, &w.owner, "doc:n", 0, json!({"workspace":d}))
        .await?
        .map_err(relations_world::msg)?;
    let related = json!({"related":{"edges":[{"to":"doc:n"}]}});
    saved(&w, "doc:u", 0, json!({"relations":related})).await?;
    let revision = read(&w, &p, "doc:u").await?["remote_links_revision"].clone();
    let links = json!([
        remote(&d, "related", false),
        remote(&d, "depends_on", true),
        remote(&k, "depends_on", true)
    ]);
    write(
        &w,
        &p,
        json!({"op":"set_remote_links","target":"doc:u","revision":revision,
        "links":links,"reason":"Uses the API"}),
    )
    .await?;
    let before = read(&w, &p, "doc:u").await?;
    let stored = before["remote_links"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(stored.len() == 3, "{before}");
    for (item, sent) in stored.iter().zip(links.as_array().into_iter().flatten()) {
        let kept = item["relation"] == sent["relation"] && item["blocking"] == sent["blocking"];
        ensure!(kept, "set_remote_links keeps relation and blocking: {item}");
    }
    let node = read(&w, &p, "doc:n").await?["document"]["revision"].clone();
    write(
        &w,
        &d,
        json!({"op":"delete_workspace","reason":"Owner order"}),
    )
    .await?;
    let after = read(&w, &p, "doc:u").await?;
    let left = after["remote_links"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(
        left.len() == 1
            && left[0]["workspace"] == k
            && left[0]["relation"] == "depends_on"
            && left[0]["blocking"] == true,
        "P keeps only the remote link to K: {after}"
    );
    ensure!(
        edges(&after, "related") == [to("doc:n")],
        "the local edge stays: {after}"
    );
    let n = read(&w, &p, "doc:n").await?;
    let document = &n["document"];
    ensure!(
        document["data"].get("workspace").is_none()
            && document["revision"].as_u64() == node.as_u64().map(|r| r + 1)
            && document["actor"] == "joppa-deletion",
        "doc:n loses only its binding: {n}"
    );
    Ok(())
}
