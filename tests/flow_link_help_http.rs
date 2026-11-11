//! ac:kov2g7y8 (requirement:board-graph): joppa_help with section objects names the Flow link of
//! one flow and the node parameter that picks one node, next to the board link of an object.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::json;

// ac:kov2g7y8
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn joppa_help_teaches_the_flow_link_of_a_node() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let help = w
        .mcp(&w.bot, "joppa_help", json!({"section":"objects"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let objects = help["objects"].as_str().unwrap_or_default();
    ensure!(
        !objects.is_empty(),
        "joppa_help gives no objects text: {help}"
    );
    let reply = help.to_string();
    for text in [
        "/?workspace=WORKSPACE&view=flow&relation=KEY",
        "&node=ADDRESS",
        "public item ID",
        "/?workspace=WORKSPACE&object=ADDRESS",
    ] {
        ensure!(
            reply.contains(text),
            "joppa_help does not give {text:?}: {reply}"
        );
    }
    Ok(())
}
