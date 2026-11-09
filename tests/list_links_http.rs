//! requirement:read-contract over a real server: a list read with `fields` that name
//! `relations` gives the stored relations of each item, over MCP and HTTP. A read by address
//! gives its detail edges.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relation_edges.rs"]
mod relation_edges;
use anyhow::{Result, ensure};
use relation_edges::targets;
use serde_json::{Value, json};

// list-links-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_item_gives_its_stored_links_over_mcp_and_http() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let save = json!({"op":"save_object","kind":"requirement","id":"s","revision":0,"parent":null,
        "data":{"title":"S","owner":"owner","body":"Second rule"},
        "relations":{"depends_on":{"edges":[{"to":"requirement:r"}]}},"reason":"Add"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    let of = |list: &Value, address: &str| {
        let items = list["items"].as_array().cloned().unwrap_or_default();
        items.into_iter().find(|i| i["address"] == address)
    };
    // An item gives only its address and the stored relations, and r gives an empty map.
    let item_s = json!({"address":"requirement:s",
        "relations":{"depends_on":{"edges":[{"to":"requirement:r"}]}}});
    let item_r = json!({"address":"requirement:r","relations":{}});
    let args = json!({"parent":"*","kind":"requirement","fields":["relations"]});
    let mcp = w.mcp(&w.bot, "joppa_objects", args).await?;
    let mcp = mcp.map_err(anyhow::Error::msg)?;
    ensure!(
        of(&mcp, "requirement:s") == Some(item_s.clone())
            && of(&mcp, "requirement:r") == Some(item_r.clone()),
        "{mcp}"
    );
    let url = format!(
        "{}/api/workspaces/{}/objects?parent=*&kind=requirement&fields=relations",
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
        of(&http, "requirement:s") == Some(item_s) && of(&http, "requirement:r") == Some(item_r),
        "{http}"
    );
    // The read by address keeps its detail edges, with the title of the target.
    let one = json!({"address":"requirement:s","fields":["relations"]});
    let one = w.mcp(&w.bot, "joppa_objects", one).await?;
    let one = one.map_err(anyhow::Error::msg)?;
    ensure!(
        targets(&one, "depends_on") == ["requirement:r"]
            && one["relations"]["depends_on"]["edges"][0]["title"] == "R",
        "{one}"
    );
    Ok(())
}
