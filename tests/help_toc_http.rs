//! joppa_help over a real server: without section or op it gives a short table of contents,
//! section gives one section, kind narrows the examples, and op gives the shape of one
//! operation with every option.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

fn keys(v: &Value) -> Vec<String> {
    v.as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

// ac:t31diuae
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn joppa_help_gives_a_short_table_of_contents_and_one_part_on_request() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let help = async |args: Value| w.mcp(&w.bot, "joppa_help", args).await;
    // The table of contents: one line for each section, the operation names and a hint.
    let toc = help(json!({})).await?.map_err(anyhow::Error::msg)?;
    ensure!(
        keys(&toc) == ["hint", "ops", "sections"]
            && keys(&toc["sections"])
                == [
                    "examples",
                    "objects",
                    "operations",
                    "process",
                    "search",
                    "writing"
                ]
            && toc["ops"]
                .as_array()
                .is_some_and(|ops| ops.contains(&json!("save_object")))
            && toc.to_string().len() < 8 * 1024,
        "{toc}"
    );
    // One section: only its key, and no field_hint.
    for (section, check) in [
        ("operations", "save_object"),
        ("writing", "ASD-STE100"),
        ("objects", "question"),
        ("search", "query"),
        ("process", "11111"),
        ("examples", "requirement"),
    ] {
        let one = help(json!({"section":section}))
            .await?
            .map_err(anyhow::Error::msg)?;
        ensure!(
            keys(&one) == [section] && one[section].to_string().contains(check),
            "{section}: {one}"
        );
    }
    // kind narrows the examples to one type.
    let ac = help(json!({"section":"examples","kind":"ac"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        keys(&ac) == ["examples"] && keys(&ac["examples"]) == ["ac"],
        "{ac}"
    );
    // One operation: its required fields, callers and options.
    let op = help(json!({"op":"save_object"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let shape = &op["operations"]["save_object"];
    ensure!(
        keys(&op) == ["operations"]
            && keys(&op["operations"]) == ["save_object"]
            && shape["required"].is_array()
            && shape["callers"].is_array()
            && shape["optional"].is_object(),
        "{op}"
    );
    let same = help(json!({"section":"operations","op":"save_object"})).await?;
    ensure!(same.as_ref() == Ok(&op), "{same:?}");
    for (args, error) in [
        (json!({"op":"nope"}), "unknown operation nope"),
        (
            json!({"section":"writing","op":"save_object"}),
            "op reads the operations section",
        ),
        (
            json!({"section":"nope"}),
            "unknown section nope; read joppa_help without section for the list",
        ),
        (
            json!({"section":"tree"}),
            "section tree was removed; use joppa_tree",
        ),
        (
            json!({"section":"examples","kind":"nope"}),
            "unknown type nope",
        ),
    ] {
        let reply = help(args.clone()).await?;
        ensure!(reply == Err(error.into()), "{args}: {reply:?}");
    }
    // Without a workspace, the section examples is refused.
    let args = json!({"section":"examples"});
    let (_, reply) = w.mcp_raw(Some(&w.bot), "joppa_help", args).await?;
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    ensure!(
        reply["result"]["isError"] == true && text == "section examples needs workspace",
        "examples: {reply}"
    );
    // No reply of the table of contents or of a section gives field_hint.
    for args in [
        json!({}),
        json!({"section":"operations"}),
        json!({"op":"note"}),
    ] {
        let reply = help(args.clone()).await?.map_err(anyhow::Error::msg)?;
        ensure!(reply.get("field_hint").is_none(), "{args}: {reply}");
    }
    Ok(())
}

/// The lowercase text of one option of save_object. It must be a non-empty text.
fn option(optional: &Value, key: &str) -> Result<String> {
    let text = optional[key].as_str().unwrap_or_default();
    ensure!(!text.trim().is_empty(), "optional.{key}: {optional}");
    Ok(text.to_lowercase())
}

// ac:yi3q532u
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn help_op_save_object_names_every_option() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let help = w
        .mcp(&w.bot, "joppa_help", json!({"op":"save_object"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let save = &help["operations"]["save_object"];
    ensure!(
        save["required"] == json!(["kind", "revision", "data", "reason"])
            && save["callers"] == json!(["member"]),
        "{save}"
    );
    let optional = &save["optional"];
    ensure!(
        keys(optional) == ["clean", "data", "id", "parent", "relations"],
        "the options of save_object: {optional}"
    );
    for key in ["clean", "data", "id", "parent", "relations"] {
        option(optional, key)?;
    }
    // id: a new object omits it, and an edit keeps it.
    let id = option(optional, "id")?;
    ensure!(
        id.contains("omit") && (id.contains("keep") || id.contains("preserve")),
        "id: {id}"
    );
    // parent: an update without parent keeps the parent.
    let parent = option(optional, "parent")?;
    ensure!(
        parent.contains("update")
            && parent.contains("without")
            && (parent.contains("keep") || parent.contains("stay")),
        "parent: {parent}"
    );
    // clean: true replaces data whole.
    let clean = option(optional, "clean")?;
    ensure!(
        clean.contains("true") && clean.contains("replace") && clean.contains("whole"),
        "clean: {clean}"
    );
    // data: an update merges data by key, and null removes a field.
    let data = option(optional, "data")?;
    ensure!(
        data.contains("update")
            && data.contains("merge")
            && data.contains("key")
            && data.contains("null")
            && data.contains("remove"),
        "data: {data}"
    );
    Ok(())
}
