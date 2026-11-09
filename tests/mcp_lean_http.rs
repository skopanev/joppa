//! requirement:read-contract over a real server: each MCP reply gives no workspace, no
//! snapshot position, no echo of the request and no default value.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

fn absent(value: &Value, keys: &[&str]) -> bool {
    keys.iter().all(|k| value.get(*k).is_none())
}

/// The keys that no read gives at its top level.
const ECHOES: [&str; 3] = ["workspace", "position", "address"];

// read-lean-replies
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_mcp_reply_gives_no_echo_and_no_default_value() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let task = json!({"title":"Tasks","role":"task","status":"none","parents":["ac"]});
    let tree = read(&w, "joppa_tree", json!({})).await?;
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "put_types":{"task":task},"reason":"Add a task type"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    for command in [
        json!({"op":"save_object","kind":"requirement","id":"s","revision":0,"parent":null,
            "data":{"title":"S","owner":"owner","body":"Second rule"},
            "relations":{"related":{"edges":[{"to":"requirement:r"}]}},"reason":"Link"}),
        json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"ac:a",
            "data":{"title":"T","owner":"owner","body":"Do A"},
            "reason":"Work"}),
    ] {
        w.command(&w.bot, command)
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    let pass = json!([{"test":"tests/x.rs > t","status":"pass"}]);
    let record = w.recorded(&"a".repeat(40), pass).await?;
    ensure!(absent(&record, &["replayed"]), "{record}");
    // The tree: one revision, no write header, no status rule copy, no false type flag.
    let tree = read(&w, "joppa_tree", json!({})).await?;
    let schema = &tree["schema"];
    ensure!(
        absent(
            &tree,
            &["workspace", "position", "current_revision", "workspace_key"]
        ) && schema["revision"].is_u64()
            && absent(schema, &["actor", "actor_type", "at", "position", "reason"])
            && absent(schema, &["status_rules"])
            && schema["types"]
                .as_object()
                .is_some_and(|types| types.values().all(|t| {
                    ["inbox", "launch_blocker", "root"]
                        .iter()
                        .all(|k| t[*k] != false)
                })),
        "{tree}"
    );
    let help = read(&w, "joppa_help", json!({"section":"tree"})).await?;
    ensure!(help["tree"] == tree, "{}", help["tree"]);
    // Object reads: no echo of the address; a revision read gives the flat document.
    let list = read(&w, "joppa_objects", json!({"parent":"requirement:r"})).await?;
    let detail = read(&w, "joppa_objects", json!({"address":"ac:a"})).await?;
    let old = read(&w, "joppa_objects", json!({"address":"ac:a","revision":1})).await?;
    let headers = read(
        &w,
        "joppa_objects",
        json!({"address":"ac:a","history":true}),
    )
    .await?;
    ensure!(
        absent(&list, &ECHOES) && list["items"].is_array() && absent(&detail, &ECHOES),
        "{list} {detail}"
    );
    ensure!(
        old["revision"] == 1
            && old["data"]["title"] == "A"
            && old["current_revision"].is_u64()
            && absent(&old, &ECHOES)
            && absent(&old, &["document", "historical", "actor", "at", "reason"])
            && absent(&old, &["schema_revision", "present"]),
        "{old}"
    );
    ensure!(
        absent(&headers, &ECHOES) && headers["items"][0]["actor"] == "bot",
        "{headers}"
    );
    // A relations_of item always names its relation (R16) and never a blocking flag.
    let edges = read(&w, "joppa_objects", json!({"relations_of":"requirement:r"})).await?;
    let edge = &edges["items"][0];
    ensure!(
        absent(&edges, &ECHOES)
            && edge["address"] == "requirement:s"
            && edge["relation"] == "related"
            && absent(edge, &["blocking"]),
        "{edges}"
    );
    // A search hit names its address and gives the clean text of the object.
    let search = read(&w, "joppa_search", json!({"query":"Second rule"})).await?;
    let hit = &search["items"][0];
    ensure!(
        absent(&search, &ECHOES)
            && absent(&search, &["embedded", "indexed", "mode", "semantic_status"])
            && hit["address"] == "requirement:s"
            && hit["excerpt"] == "Second rule"
            && absent(hit, &["kind", "id", "archived", "historical", "exact_id"]),
        "{search}"
    );
    // History gives no default flag and no echo.
    // The test node of A gives its tone in the list and no echo in the detail.
    let tests = read(&w, "joppa_objects", json!({"parent":"ac:a","kind":"test"})).await?;
    let node = &tests["items"][0];
    let address = node["address"].as_str().unwrap_or_default();
    let detail = read(&w, "joppa_objects", json!({"address":address})).await?;
    ensure!(
        absent(&tests, &ECHOES)
            && node["status"] == "green"
            && absent(node, &["status_name", "tests_draft", "tests_saved_by"])
            && absent(&detail, &ECHOES)
            && absent(&detail, &["tests_draft", "tests_saved_by"]),
        "{tests} {detail}"
    );
    let page = read(&w, "joppa_history", json!({})).await?;
    ensure!(absent(&page, &ECHOES) && page["items"].is_array(), "{page}");
    // A write gives the saved object and only the op of its command; a retry says replayed.
    let save = json!({"op":"save_object","kind":"requirement","id":"u","revision":0,
        "parent":null,"data":{"title":"U","owner":"owner","body":"Third rule"},"reason":"Add"});
    let args = json!({"request_id":"lean-write","command":save});
    let first = read(&w, "joppa_command", args.clone()).await?;
    let again = read(&w, "joppa_command", args).await?;
    ensure!(
        first["command"] == json!({"op":"save_object"})
            && first["object"]["address"] == "requirement:u"
            && first["position"].is_u64()
            && absent(&first, &["replayed"])
            && again["replayed"] == true,
        "{first} {again}"
    );
    // A file write and the file list give the file and nothing that the request named.
    let file = json!({"target":"ac:a","filename":"a.txt","content":"Evidence"});
    let attached = read(&w, "joppa_attach", file).await?;
    let files = read(&w, "joppa_attachments", json!({"target":"ac:a"})).await?;
    ensure!(
        attached["file"]["name"] == "a.txt"
            && absent(&attached, &["workspace", "target", "position", "replayed"])
            && files["files"][0]["name"] == "a.txt"
            && absent(&files, &["workspace", "target"]),
        "{attached} {files}"
    );
    Ok(())
}
