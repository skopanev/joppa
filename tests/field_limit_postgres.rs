//! requirement:field-length-limit over a real server and its database: a save refuses a new
//! value over the default 1024 characters, the database trigger refuses a value over the
//! ceiling of 262144 characters, a replay or a rebuild may write it, a stored long value may
//! stay, and the MCP tools name the limit.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const LIMIT_TEXT: &str = "at most 1024 characters";
const DB_LIMIT_TEXT: &str = "at most 262144 characters";
const DB_OVER: usize = 262145;

fn requirement(id: &str, body: &Value) -> Value {
    json!({"op":"save_object","kind":"requirement","id":id,"revision":0,"parent":null,
        "data":{"title":"Field limit","owner":"owner","body":body},"reason":"Field limit test"})
}

// field-limit-save
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_save_refuses_a_new_value_over_1024_characters() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let at_limit = json!("я".repeat(1024));
    let saved = w
        .command(&w.bot, requirement("at-limit", &at_limit))
        .await?;
    ensure!(saved.is_ok(), "1024 characters were refused: {saved:?}");
    for (id, body) in [
        ("over", json!("я".repeat(1025))),
        ("nested", json!(["short", "x".repeat(1025)])),
    ] {
        let refused = w.command(&w.bot, requirement(id, &body)).await?;
        let error = refused.err().unwrap_or_default();
        ensure!(error.contains(LIMIT_TEXT), "{id}: {error}");
    }
    // A long description is a field of data too.
    let described = w
        .command(&w.bot, json!({"op":"save_object","kind":"requirement","id":"at-limit","revision":1,"parent":null,
            "data":{"title":"Field limit","owner":"owner","body":at_limit,"description":"x".repeat(1025)},"reason":"Too long"}))
        .await?;
    ensure!(
        described.err().unwrap_or_default().contains(LIMIT_TEXT),
        "a description over 1024 characters passed"
    );
    Ok(())
}

// field-limit-database
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_database_refuses_a_new_long_field_unless_it_replays() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let mut conn = w.pool.get().await?;
    let long = json!({"data":{"title":"Long","description":"z".repeat(DB_OVER)}});
    let insert = "INSERT INTO joppa.object_revisions(workspace,address,revision,document) VALUES($1,'note:direct',$2,$3::text::jsonb)";
    let refused = conn
        .execute(insert, &[&w.ws, &1i32, &long.to_string()])
        .await;
    let message = format!("{:?}", refused.err());
    ensure!(message.contains(DB_LIMIT_TEXT), "{message}");
    // A replay or a rebuild sets joppa.replaying and writes the journal as it is.
    let tx = conn.transaction().await?;
    tx.batch_execute("SET LOCAL joppa.replaying = '1'").await?;
    tx.execute(insert, &[&w.ws, &1i32, &long.to_string()])
        .await?;
    tx.commit().await?;
    // A later revision may keep the stored long value and change another field.
    let kept = json!({"data":{"title":"Changed","description":"z".repeat(DB_OVER)}});
    conn.execute(insert, &[&w.ws, &2i32, &kept.to_string()])
        .await?;
    // A changed long value is refused again.
    let changed = json!({"data":{"title":"Changed","description":"y".repeat(DB_OVER)}});
    let refused = conn
        .execute(insert, &[&w.ws, &3i32, &changed.to_string()])
        .await;
    ensure!(
        format!("{:?}", refused.err()).contains(DB_LIMIT_TEXT),
        "a changed long value passed"
    );
    // The trigger holds the ceiling; the server checks the field_max_length of each type.
    let typed = json!({"data":{"title":"Typed","description":"y".repeat(4096)}});
    conn.execute(insert, &[&w.ws, &3i32, &typed.to_string()])
        .await?;
    // The current row of an object has the same trigger.
    let update = "UPDATE joppa.objects SET document = jsonb_set(document, '{data,body}', to_jsonb($2::text)) WHERE workspace = $1 AND address = 'requirement:r'";
    let refused = conn.execute(update, &[&w.ws, &"w".repeat(DB_OVER)]).await;
    ensure!(
        format!("{:?}", refused.err()).contains(DB_LIMIT_TEXT),
        "a long field passed on joppa.objects"
    );
    let tx = conn.transaction().await?;
    tx.batch_execute("SET LOCAL joppa.replaying = '1'").await?;
    let rebuilt = tx.execute(update, &[&w.ws, &"w".repeat(DB_OVER)]).await?;
    tx.commit().await?;
    ensure!(rebuilt == 1, "the rebuild did not write joppa.objects");
    Ok(())
}

// field-limit-help
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_write_tools_and_the_save_examples_name_the_limit() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    for name in ["joppa_command", "joppa_batch"] {
        let tool = reply["result"]["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|t| t["name"] == name))
            .cloned()
            .unwrap_or_default();
        let text = tool["description"].as_str().unwrap_or_default();
        ensure!(
            text.contains("at most the field_max_length of its type (default 1024 characters)")
                && text.contains(joppa::field_limit::HINT),
            "{name} does not name the limit"
        );
    }
    let help = w.mcp(&w.bot, "joppa_help", json!({})).await?; // mcp() adds the workspace
    let help = help.map_err(|e| anyhow::anyhow!(e))?;
    ensure!(
        help["examples"]["requirement"]["data"]["body"] == "<concise text (max 1024 chars)>",
        "{}",
        help["examples"]["requirement"]
    );
    Ok(())
}

// field-limit-help
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_write_tools_and_the_help_send_links_and_comments_out_of_object_text() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tools = reply["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for name in ["joppa_command", "joppa_batch"] {
        let tool = tools
            .iter()
            .find(|t| t["name"] == name)
            .cloned()
            .unwrap_or_default();
        let text = tool["description"].as_str().unwrap_or_default();
        ensure!(
            text.contains("links array of save_object")
                && text.contains(r#"{"op":"note","target":"kind:id""#),
            "{name} does not send links and comments out of object text"
        );
    }
    let command = tools
        .iter()
        .find(|t| t["name"] == "joppa_command")
        .cloned()
        .unwrap_or_default();
    ensure!(
        command["inputSchema"]["properties"]["command"]["properties"]["body"]["description"]
            .as_str()
            .is_some_and(|d| d.contains("use links and the note op")),
        "the text field schema does not name links and the note op"
    );
    let help = w.mcp(&w.bot, "joppa_help", json!({})).await?;
    let help = help.map_err(|e| anyhow::anyhow!(e))?;
    let objects = help["objects"].as_str().unwrap_or_default();
    ensure!(
        objects.contains("## Links and comments, not object text")
            && objects.contains(r#"{"op": "note", "target": "<kind>:<id>""#)
            && objects.contains("Do not paste lists of Tasks, tickets or addresses"),
        "joppa_help.objects does not state the two rules"
    );
    Ok(())
}
