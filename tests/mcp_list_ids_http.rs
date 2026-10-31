//! requirement:read-contract over a real server: a list item is {address, title, status} and
//! names its children only on request, a new object gets an 8-character ID, and the MCP reads
//! take a public item ID wherever they take an address.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

async fn ok(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

// read-list-default
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_item_gives_address_title_and_status_and_children_on_request() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let roots = ok(&w, "joppa_objects", json!({})).await?;
    let r = roots["items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| i["address"] == "requirement:r"))
        .cloned()
        .context("requirement:r")?;
    let keys: Vec<&String> = r
        .as_object()
        .map(|m| m.keys().collect())
        .unwrap_or_default();
    ensure!(keys == ["address", "status", "title"], "{r}");
    let fields = json!({"fields":["title","children"]});
    let counted = ok(&w, "joppa_objects", fields).await?;
    let one = ok(
        &w,
        "joppa_objects",
        json!({"address":"requirement:r","fields":["children"]}),
    )
    .await?;
    ensure!(
        counted["items"].as_array().is_some_and(|items| items
            .iter()
            .any(|i| i["address"] == "requirement:r" && i["children"] == 1))
            && one == json!({"children":1}),
        "{counted} {one}"
    );
    Ok(())
}

// short-object-ids
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_object_gets_8_characters_and_a_public_id_reads_like_its_address() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let save = json!({"op":"save_object","kind":"requirement","revision":0,"parent":null,
        "data":{"title":"Short","owner":"owner","body":"A short ID"},"reason":"Add"});
    let saved = w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    let address = saved["object"]["address"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let id = address.strip_prefix("requirement:").unwrap_or_default();
    ensure!(
        id.len() == 8
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
        "{saved}"
    );
    let conn = w.pool.get().await?;
    let item = |kind: &str, local: &str| {
        let (kind, local) = (kind.to_owned(), local.to_owned());
        let conn = &conn;
        let ws = w.ws.clone();
        async move {
            let row = conn
                .query_one(
                    "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind=$2 AND local_id=$3",
                    &[&ws, &kind, &local],
                )
                .await?;
            anyhow::Ok(row.get::<_, String>(0))
        }
    };
    let (ac, r) = (item("ac", "a").await?, item("requirement", "r").await?);
    // The public ID reads the same object; the reply names the address it resolved.
    let by_item = ok(&w, "joppa_objects", json!({"address":ac})).await?;
    let by_address = ok(&w, "joppa_objects", json!({"address":"ac:a"})).await?;
    let mut named = by_address.clone();
    named["address"] = json!("ac:a");
    let children = ok(&w, "joppa_objects", json!({"parent":r})).await?;
    let tests = ok(&w, "joppa_tests", json!({"ac":ac})).await?;
    ensure!(
        by_item == named
            && children["items"][0]["address"] == "ac:a"
            && tests["tests"][0]["address"] == "tests/x.rs > t",
        "{by_item} {children} {tests}"
    );
    let unknown = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"address":"nope-R-NOPE0000"}),
        )
        .await?;
    ensure!(unknown == Err("unknown object".into()), "{unknown:?}");
    Ok(())
}
