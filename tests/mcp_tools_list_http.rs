//! requirement:mcp-tools over a real server: tools/list gives the writing rules once, short
//! text field descriptions, no page or file-size hint in each tool, a smaller reply, and a
//! short command schema of joppa_batch. initialize gives the page hint.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// The raw body of one JSON-RPC call on POST /mcp with the token of bot.
async fn raw(w: &record_world::World, method: &str) -> Result<String> {
    let reply = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}}))
        .send()
        .await?
        .error_for_status()?;
    Ok(reply.text().await?)
}

/// The tools of a raw tools/list reply.
fn tools(body: &str) -> Result<Vec<Value>> {
    let reply: Value = serde_json::from_str(body)?;
    reply["result"]["tools"]
        .as_array()
        .cloned()
        .with_context(|| format!("no tools: {reply}"))
}

fn tool<'a>(tools: &'a [Value], name: &str) -> Result<&'a Value> {
    tools
        .iter()
        .find(|t| t["name"] == name)
        .with_context(|| format!("{name} is not listed"))
}

/// The paths of each object key named `key` in `value`.
fn keys_named(value: &Value, key: &str, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let here = format!("{path}.{k}");
                if k == key {
                    found.push(here.clone());
                }
                keys_named(v, key, &here, found);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                keys_named(v, key, &format!("{path}[{i}]"), found);
            }
        }
        _ => {}
    }
}

// ac:sz1ankav
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn tools_list_gives_the_writing_rules_once_and_short_text_field_descriptions() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let body = raw(&w, "tools/list").await?;
    let rules = "in at least 80% of sentences";
    let count = body.matches(rules).count();
    ensure!(count == 1, "{rules:?} occurs {count} times in tools/list");
    let tools = tools(&body)?;
    let command = tool(&tools, "joppa_command")?;
    let description = command["description"].as_str().unwrap_or_default();
    ensure!(
        description.contains(rules),
        "the description of joppa_command does not hold {rules:?}: {description}"
    );
    let properties = &command["inputSchema"]["properties"]["command"]["properties"];
    for field in ["body", "text", "reason", "value"] {
        let text = properties[field]["description"]
            .as_str()
            .with_context(|| format!("{field} has no description: {properties}"))?;
        let length = text.chars().count();
        ensure!(
            length <= 80,
            "the description of {field} has {length} characters, more than 80: {text}"
        );
    }
    Ok(())
}

// ac:bu6ms6hi
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_page_and_file_size_hints_are_in_initialize_and_not_in_each_tool() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let body = raw(&w, "tools/list").await?;
    let hint = "Over 1 kb: prefer attach as a file";
    let count = body.matches(hint).count();
    ensure!(count <= 1, "{hint:?} occurs {count} times in tools/list");
    let tools = tools(&body)?;
    for tool in &tools {
        let text = tool["description"].as_str().unwrap_or_default();
        ensure!(
            !text.contains("next_cursor"),
            "the description of {} holds next_cursor: {text}",
            tool["name"]
        );
    }
    let page = "next_cursor of the previous page";
    ensure!(
        body.contains(page),
        "no parameter description holds {page:?}"
    );
    for name in ["joppa_objects", "joppa_search"] {
        let cursor = &tool(&tools, name)?["inputSchema"]["properties"]["cursor"];
        ensure!(
            cursor["description"]
                .as_str()
                .is_some_and(|d| d.contains(page)),
            "{name}.cursor: {cursor}"
        );
    }
    let init: Value = serde_json::from_str(&raw(&w, "initialize").await?)?;
    let instructions = init["result"]["instructions"].as_str().unwrap_or_default();
    ensure!(
        instructions.contains("next_cursor"),
        "the initialize instructions do not hold next_cursor"
    );
    Ok(())
}

// ac:unjllzrh
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn tools_list_is_at_least_4500_bytes_smaller() -> Result<()> {
    const BEFORE: usize = 23645;
    const LIMIT: usize = BEFORE - 4500;
    let w = record_world::world(&[]).await?;
    let bytes = raw(&w, "tools/list").await?.len();
    ensure!(
        bytes <= LIMIT,
        "tools/list has {bytes} bytes; before the change it had {BEFORE}, the limit is {LIMIT}"
    );
    Ok(())
}

// ac:61lt9q80
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn joppa_batch_stays_listed_with_a_short_command_schema_and_saves_two_objects() -> Result<()>
{
    let w = record_world::world(&[]).await?;
    let body = raw(&w, "tools/list").await?;
    let tools = tools(&body)?;
    let batch = tool(&tools, "joppa_batch")?;
    let command =
        &batch["inputSchema"]["properties"]["operations"]["items"]["properties"]["command"];
    let expected = json!({"type":"object","description":"One operation object, by the rules of joppa_command.command."});
    ensure!(*command == expected, "the batch command schema: {command}");
    let mut refs = vec![];
    for tool in &tools {
        let name = tool["name"].as_str().unwrap_or_default();
        keys_named(&tool["inputSchema"], "$ref", name, &mut refs);
    }
    ensure!(refs.is_empty(), "inputSchema keys named $ref: {refs:?}");
    let save = |id: &str| {
        json!({"command":{"op":"save_object","kind":"requirement","id":id,"revision":0,"parent":null,
            "data":{"title":format!("Batch {id}"),"owner":"owner","body":"A batch rule"},"reason":"Batch test"}})
    };
    let args = json!({"request_id":uuid::Uuid::new_v4().to_string(),
        "operations":[save("batch-one"),save("batch-two")]});
    let reply = w.mcp(&w.bot, "joppa_batch", args).await?;
    reply.map_err(|e| anyhow::anyhow!("joppa_batch: {e}"))?;
    for id in ["batch-one", "batch-two"] {
        let object = w.object(&format!("requirement:{id}")).await?;
        ensure!(
            object["data"]["title"] == format!("Batch {id}"),
            "requirement:{id}: {object}"
        );
    }
    Ok(())
}
