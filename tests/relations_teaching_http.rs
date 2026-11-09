//! requirement:q438uw0s over a real server: the help, the tools, the agent instructions and
//! the guide teach relations and the registry, and no longer teach a top-level links field or
//! a blocking flag on a local edge. The set_remote_links text stays.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use relations_world::msg;
use serde_json::{Value, json};

/// One JSON-RPC call to /mcp as the agent bot.
async fn rpc(w: &World, method: &str) -> Result<Value> {
    let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}});
    let reply = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&body);
    Ok(reply.send().await?.error_for_status()?.json().await?)
}

/// The phrases of the older text that teach a top-level links field of save_object.
const LINKS_FIELD: [&str; 5] = [
    "links stay",
    "links array",
    "links[]",
    "[\"links\"]",
    "Omit parent and links",
];

/// A text teaches no top-level links field, and each sentence that names a blocking flag also
/// names a remote link.
fn no_local_flag(name: &str, text: &str) -> Result<()> {
    for phrase in LINKS_FIELD {
        ensure!(
            !text.contains(phrase),
            "{name} teaches the links field: {phrase:?}"
        );
    }
    for sentence in text.split(['.', '<', '\n']) {
        let flag = [
            "\"blocking\"",
            "blocking:",
            "blocking flag",
            "blocking link",
            "blocking: true",
        ]
        .iter()
        .any(|p| sentence.contains(p));
        ensure!(
            !flag || sentence.contains("remote"),
            "{name} teaches a local blocking flag: {sentence}"
        );
    }
    Ok(())
}

// ac:r844gtkn
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn help_the_tools_the_instructions_and_the_guide_teach_relations() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    let tools = rpc(&w, "tools/list").await?;
    let text = tools.to_string();
    for word in ["relations", "put_relations", "relations_of"] {
        ensure!(text.contains(word), "tools/list does not name {word}");
    }
    let tool = |name: &str| {
        let list = tools["result"]["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        list.into_iter()
            .find(|t| t["name"] == name)
            .unwrap_or(Value::Null)
    };
    let objects = &tool("joppa_objects")["inputSchema"]["properties"];
    ensure!(
        objects.get("relations_of").is_some() && objects.get("links_of").is_none(),
        "{objects}"
    );
    let command = &tool("joppa_command")["inputSchema"]["properties"]["command"]["properties"];
    ensure!(
        command.get("relations").is_some() && command.get("blocking").is_none(),
        "{command}"
    );
    no_local_flag("tools/list", &text)?;
    let init = rpc(&w, "initialize").await?;
    let instructions = init["result"]["instructions"]
        .as_str()
        .context("instructions")?;
    ensure!(
        instructions.contains("relations"),
        "initialize does not teach relations"
    );
    for phrase in LINKS_FIELD {
        ensure!(
            !instructions.contains(phrase),
            "initialize teaches the links field: {phrase:?}"
        );
    }
    let help = async |args: Value| w.mcp(&w.bot, "joppa_help", args).await?.map_err(msg);
    let save = help(json!({"op":"save_object"})).await?;
    let examples = help(json!({"section":"examples"})).await?;
    ensure!(
        save.to_string().contains("relations") || examples.to_string().contains("relations"),
        "joppa_help does not teach relations: {save} {examples}"
    );
    let operation = &save["operations"]["save_object"];
    ensure!(
        relations_world::without_key(operation, "blocking"),
        "{save}"
    );
    let names = |key: &str| operation[key].as_array().cloned().unwrap_or_default();
    ensure!(
        !names("required").contains(&json!("links"))
            && !names("optional").contains(&json!("links")),
        "{save}"
    );
    no_local_flag("joppa_help op save_object", &save.to_string())?;
    let list = examples["examples"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    ensure!(!list.is_empty(), "{examples}");
    for (kind, example) in &list {
        if example["op"] == "save_object" {
            let right = example.get("relations").is_some() && example.get("links").is_none();
            ensure!(right, "the example of {kind}: {example}");
        }
    }
    no_local_flag("joppa_help section examples", &examples.to_string())?;
    let remote = help(json!({"op":"set_remote_links"})).await?;
    let required = &remote["operations"]["set_remote_links"]["required"];
    ensure!(
        *required == json!(["target", "revision", "links", "reason"]),
        "{remote}"
    );
    let guide = w.http.get(format!("{}/guide", w.root)).send().await?;
    ensure!(
        guide.status().is_success(),
        "GET /guide: {}",
        guide.status()
    );
    let guide = guide.text().await?;
    for word in [
        "relations",
        "put_relations",
        "relations_of",
        "set_remote_links",
    ] {
        ensure!(guide.contains(word), "the guide does not name {word}");
    }
    no_local_flag("the guide", &guide)
}
