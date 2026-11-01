//! joppa_help over a real server: without section or op it gives a short table of contents,
//! section gives one section, and op gives the shape of one operation.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn joppa_help_gives_a_short_table_of_contents_and_one_part_on_request() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let help = async |args: Value| w.mcp(&w.bot, "joppa_help", args).await;
    let keys = |v: &Value| -> Vec<String> {
        v.as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    };
    // The table of contents: one line for each section, the operation names and a hint.
    let toc = help(json!({})).await?.map_err(anyhow::Error::msg)?;
    ensure!(
        keys(&toc) == ["field_hint", "hint", "ops", "sections"]
            && keys(&toc["sections"])
                == [
                    "examples",
                    "objects",
                    "operations",
                    "search",
                    "tree",
                    "writing"
                ]
            && toc["ops"]
                .as_array()
                .is_some_and(|ops| ops.contains(&json!("save_object")))
            && toc.to_string().len() < 8 * 1024,
        "{toc}"
    );
    // One section: only its key.
    for (section, check) in [
        ("operations", "save_object"),
        ("writing", "ASD-STE100"),
        ("objects", "question"),
        ("search", "query"),
        ("tree", "schema"),
        ("examples", "requirement"),
    ] {
        let one = help(json!({"section":section}))
            .await?
            .map_err(anyhow::Error::msg)?;
        ensure!(
            keys(&one).len() == 2
                && one["field_hint"].is_string()
                && one[section].to_string().contains(check),
            "{section}: {one}"
        );
    }
    // One operation: its required fields and callers.
    let op = help(json!({"op":"save_object"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        keys(&op["operations"]) == ["save_object"]
            && op["operations"]["save_object"]["required"].is_array()
            && op["operations"]["save_object"]["callers"].is_array(),
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
    ] {
        let reply = help(args.clone()).await?;
        ensure!(reply == Err(error.into()), "{args}: {reply:?}");
    }
    // Without a workspace, the sections tree and examples are refused.
    for section in ["tree", "examples"] {
        let args = json!({"name":"joppa_help","arguments":{"section":section}});
        let (_, reply) = w
            .mcp_raw(Some(&w.bot), "joppa_help", args["arguments"].clone())
            .await?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        ensure!(
            reply["result"]["isError"] == true
                && text == format!("section {section} needs workspace"),
            "{section}: {reply}"
        );
    }
    Ok(())
}
