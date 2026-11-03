//! requirement:read-contract over a real server: a list read with `fields` that name `links`
//! gives the stored links of each item, over MCP and HTTP. A read by address gives its detail
//! links.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

// list-links-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_item_gives_its_stored_links_over_mcp_and_http() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let save = json!({"op":"save_object","kind":"requirement","id":"s","revision":0,"parent":null,
        "data":{"title":"S","owner":"owner","body":"Second rule"},
        "links":[{"target":"requirement:r","relation":"depends_on","blocking":true}],"reason":"Add"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    let stored = json!([{"target":"requirement:r","relation":"depends_on","blocking":true}]);
    let of = |list: &Value, address: &str| {
        let items = list["items"].as_array().cloned().unwrap_or_default();
        items.into_iter().find(|i| i["address"] == address)
    };
    let args = json!({"parent":"*","kind":"requirement","fields":["links"]});
    let mcp = w.mcp(&w.bot, "joppa_objects", args).await?;
    let mcp = mcp.map_err(anyhow::Error::msg)?;
    ensure!(
        of(&mcp, "requirement:s") == Some(json!({"address":"requirement:s","links":stored})),
        "{mcp}"
    );
    ensure!(
        of(&mcp, "requirement:r") == Some(json!({"address":"requirement:r","links":[]})),
        "{mcp}"
    );
    let url = format!(
        "{}/api/workspaces/{}/objects?parent=*&kind=requirement&fields=links",
        w.root, w.ws
    );
    let http: Value = w
        .http
        .get(url)
        .bearer_auth(&w.bot)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        of(&http, "requirement:s") == of(&mcp, "requirement:s"),
        "{http}"
    );
    // The read by address keeps its detail links, with the title of the target.
    let one = json!({"address":"requirement:s","fields":["links"]});
    let one = w.mcp(&w.bot, "joppa_objects", one).await?;
    let one = one.map_err(anyhow::Error::msg)?;
    ensure!(
        one["links"][0]["target"] == "requirement:r" && one["links"][0]["title"] == "R",
        "{one}"
    );
    Ok(())
}
