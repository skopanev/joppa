//! requirement:read-contract over a real server: the MCP tree gives no status colours, no type
//! UI fields and no default title limit. GET /api/workspaces/{w}/tree keeps them all.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// The type UI fields that only the HTTP tree and the board give.
const UI: [&str; 4] = ["color", "badge", "index", "title_size"];
/// The type keys that the MCP tree keeps as the HTTP tree gives them.
const KEPT: [&str; 7] = [
    "parents",
    "role",
    "question",
    "references",
    "fields",
    "required_fields",
    "status",
];

/// The MCP tree of the bot, at schema revision `revision` or the current one.
async fn mcp_tree(w: &record_world::World, revision: Option<u64>) -> Result<Value> {
    let args = revision.map_or(json!({}), |r| json!({"revision":r}));
    w.mcp(&w.bot, "joppa_tree", args)
        .await?
        .map_err(anyhow::Error::msg)
}

/// GET /api/workspaces/W/tree of the owner.
async fn http_tree(w: &record_world::World) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}/tree", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.owner).send().await?;
    let code = reply.status();
    let tree: Value = reply.json().await?;
    ensure!(code.is_success(), "GET tree: {code} {tree}");
    Ok(tree)
}

/// The MCP schema has no colours, priorities ["p0","p1","p2"] and no priority_levels. No type
/// has a UI field. A type gives title_max_length only for a limit other than the default 128.
/// Each type keeps the keys of KEPT as `http` gives them.
fn lean(name: &str, mcp: &Value, http: &Value) -> Result<()> {
    let schema = &mcp["schema"];
    ensure!(
        ["status_colors", "statuses", "priority_levels"]
            .iter()
            .all(|k| schema.get(*k).is_none())
            && schema["priorities"] == json!(["p0", "p1", "p2"]),
        "{name}: {schema}"
    );
    let types = schema["types"].as_object().cloned().unwrap_or_default();
    ensure!(types.contains_key("ac"), "{name}: {schema}");
    for (kind, t) in &types {
        ensure!(UI.iter().all(|k| t.get(*k).is_none()), "{name} {kind}: {t}");
        let source = &http["schema"]["types"][kind];
        for key in KEPT {
            ensure!(
                t.get(key) == source.get(key),
                "{name} {kind} {key}: {t} {source}"
            );
        }
        let limit = source
            .get("title_preview_length")
            .or(source.get("title_max_length"))
            .filter(|l| **l != 128);
        ensure!(
            t.get("title_max_length") == limit,
            "{name} {kind}: {t} {source}"
        );
    }
    Ok(())
}

// ac:bwc9qvme
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_mcp_tree_gives_no_colours_ui_fields_or_default_title_limit() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    let revision = mcp_tree(&w, None).await?["schema"]["revision"].clone();
    let ac = json!({"title":"Criteria","role":"ac","status":"children","parents":["requirement"],
        "color":"#63756b","badge":"AC","index":0,"title_size":14,"title_max_length":128});
    let statuses = json!([{"tone":"green","light":"#22a559","dark":"#2fbf6a"},
        {"tone":"blue","light":"#6fa8dc","dark":"#8bbbe8"}]);
    let schema = json!({"op":"set_tree_schema","revision":revision,"put_types":{"ac":ac},
        "statuses":statuses,"reason":"Colour the criteria"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    // An older release stored the status_colors map: the replay of its event keeps it.
    w.stop();
    let colours = json!({"status_colors":{"red":"#aa0000"}});
    blue_world::old_schema(&w, &w.ws, colours).await?;
    w.start().await?;
    let http = http_tree(&w).await?;
    let schema = &http["schema"];
    let t = &schema["types"]["ac"];
    let limit = t.get("title_preview_length").or(t.get("title_max_length"));
    ensure!(
        schema["statuses"].is_array()
            && schema["status_colors"].is_object()
            && schema["priority_levels"] == json!(["p0", "p1", "p2"])
            && t["color"] == "#63756b"
            && t["badge"] == "AC"
            && t["index"] == 0
            && t["title_size"] == 14
            && limit == Some(&json!(128)),
        "HTTP: {http}"
    );
    let current = mcp_tree(&w, None).await?;
    lean("current", &current, &http)?;
    let types = &current["schema"]["types"];
    ensure!(
        types["ac"].get("title_max_length").is_none() && types["test"]["title_max_length"] == 400,
        "current: {current}"
    );
    // The stored revision 1 reads by the same rules. Its HTTP read is the source.
    let url = format!("{}/api/workspaces/{}/tree?revision=1", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.owner).send().await?;
    let first: Value = reply.error_for_status()?.json().await?;
    lean("revision 1", &mcp_tree(&w, Some(1)).await?, &first)?;
    Ok(())
}
