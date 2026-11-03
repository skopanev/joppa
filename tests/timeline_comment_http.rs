//! joppa_timeline over a real server: an entry of a comment event names its comment in
//! comment_id, for the ops note and withdraw_note.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_comment_entry_of_the_timeline_names_its_comment() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    for command in [
        json!({"op":"note","target":"ac:a","id":"c1","text":"A question"}),
        json!({"op":"withdraw_note","target":"ac:a","id":"c1","reason":"Asked twice"}),
    ] {
        w.command(&w.bot, command.clone())
            .await?
            .map_err(|e| anyhow::anyhow!("{command}: {e}"))?;
    }
    let newest = json!({"order":"newest","limit":2});
    let page = w.mcp(&w.alex, "joppa_timeline", newest).await?;
    let page = page.map_err(anyhow::Error::msg)?;
    let entries: Vec<(Value, Value, Value)> = page["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            (
                i["op"].clone(),
                i["change"].clone(),
                i["comment_id"].clone(),
            )
        })
        .collect();
    ensure!(
        entries
            == [
                (json!("withdraw_note"), json!("comment"), json!("c1")),
                (json!("note"), json!("comment"), json!("c1")),
            ],
        "{page}"
    );
    // Another change has no comment_id.
    let all = w
        .mcp(&w.alex, "joppa_timeline", json!({"limit":25}))
        .await?;
    let all = all.map_err(anyhow::Error::msg)?;
    let other = all["items"].as_array().into_iter().flatten();
    ensure!(
        other
            .filter(|i| i["change"] != "comment")
            .all(|i| i.get("comment_id").is_none()),
        "{all}"
    );
    Ok(())
}
