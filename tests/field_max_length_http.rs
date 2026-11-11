//! requirement:field-length-limit over a real server: a type sets field_max_length from 1 to
//! 262144 characters, a save of that type uses it, other types keep 1024, the initialize
//! instructions and /tree give the advice "Over 1 kb: prefer attach as a file", and no
//! joppa_tree or joppa_help reply gives field_hint.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use joppa::field_limit::HINT;
use serde_json::{Value, json};

fn ok(reply: Result<Value, String>) -> Result<Value> {
    reply.map_err(anyhow::Error::msg)
}

/// Save the tree schema with field_max_length `length` on the requirement type.
async fn set_length(w: &record_world::World, length: u32) -> Result<Result<Value, String>> {
    let tree = ok(w.mcp(&w.owner, "joppa_tree", json!({})).await?)?;
    let mut types = tree["schema"]["types"].clone();
    types["requirement"]["field_max_length"] = json!(length);
    let revision = &tree["schema"]["revision"];
    w.command(
        &w.owner,
        json!({"op":"set_tree_schema","revision":revision,"types":types,
        "reason":"Field length of requirements"}),
    )
    .await
}

fn requirement(id: &str, body: String) -> Value {
    json!({"op":"save_object","kind":"requirement","id":id,"revision":0,"parent":null,
        "data":{"title":"Typed limit","owner":"owner","body":body},"reason":"Typed limit test"})
}

// ac:field-max-length
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_type_sets_its_field_max_length_up_to_262144_characters() -> Result<()> {
    let w = record_world::world(&[]).await?;
    for length in [0, 262145] {
        let refused = set_length(&w, length).await?.err().unwrap_or_default();
        ensure!(
            refused
                .contains("type field_max_length must be an integer from 1 to 262144 characters"),
            "{length}: {refused}"
        );
    }
    ok(set_length(&w, 4096).await?)?;
    let tree = ok(w.mcp(&w.bot, "joppa_tree", json!({})).await?)?;
    ensure!(
        tree["schema"]["types"]["requirement"]["field_max_length"] == 4096,
        "{tree}"
    );
    ensure!(tree.get("field_hint").is_none(), "{tree}");
    ok(w.command(&w.bot, requirement("typed", "я".repeat(4096)))
        .await?)?;
    let over = w
        .command(&w.bot, requirement("over", "я".repeat(4097)))
        .await?;
    let error = over.err().unwrap_or_default();
    ensure!(
        error.contains("at most 4096 characters") && error.contains(HINT),
        "{error}"
    );
    // A type without field_max_length keeps 1024 characters.
    let domain = json!({"op":"save_object","kind":"domain","id":"long","revision":0,"parent":null,
        "data":{"title":"Long domain","owner":"owner","body":"x".repeat(1025)},"reason":"Default limit"});
    let error = w.command(&w.bot, domain).await?.err().unwrap_or_default();
    ensure!(error.contains("at most 1024 characters"), "{error}");
    // The ceiling: 262144 characters pass the server and the database trigger.
    ok(set_length(&w, 262144).await?)?;
    ok(
        w.command(&w.bot, requirement("ceiling", "z".repeat(262144)))
            .await?,
    )?;
    let over = w
        .command(&w.bot, requirement("past", "z".repeat(262145)))
        .await?;
    ensure!(
        over.err()
            .unwrap_or_default()
            .contains("at most 262144 characters")
    );
    Ok(())
}

// ac:field-limit-hint
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn initialize_and_the_tree_page_say_over_1_kb_prefer_attach_as_a_file_and_no_reply_gives_field_hint()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let instructions = reply["result"]["instructions"]
        .as_str()
        .context("instructions")?;
    ensure!(
        instructions.contains(HINT),
        "the initialize instructions do not hold the hint"
    );
    // No joppa_tree or joppa_help reply gives field_hint: joppa_help without section and with
    // each section of its table of contents.
    let tree = ok(w.mcp(&w.bot, "joppa_tree", json!({})).await?)?;
    ensure!(tree.get("field_hint").is_none(), "joppa_tree: {tree}");
    let toc = ok(w.mcp(&w.bot, "joppa_help", json!({})).await?)?;
    let sections: Vec<String> = toc["sections"]
        .as_object()
        .context("sections")?
        .keys()
        .cloned()
        .collect();
    ensure!(!sections.is_empty(), "{toc}");
    ensure!(toc.get("field_hint").is_none(), "joppa_help: {toc}");
    for section in sections {
        let read = ok(w
            .mcp(&w.bot, "joppa_help", json!({"section":section}))
            .await?)?;
        ensure!(read.get("field_hint").is_none(), "{section}: {read}");
    }
    let page = reqwest::get(format!("{}/tree", w.root))
        .await?
        .text()
        .await?;
    ensure!(
        page.contains(&format!(r#"<p class="hint" id="field-hint">{HINT}</p>"#)),
        "/tree has no field hint"
    );
    Ok(())
}
