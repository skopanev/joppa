//! requirement:read-contract over a real server: the `data` filter of joppa_objects lists the
//! objects whose document.data contains a JSON object, at every level of the tree.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, args: Value) -> Result<Result<Value, String>> {
    w.mcp(&w.bot, "joppa_objects", args).await
}

fn addresses(page: &Value) -> Vec<&str> {
    let items = page["items"].as_array().into_iter().flatten();
    items.filter_map(|i| i["address"].as_str()).collect()
}

// read-data-filter
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_data_filter_lists_the_objects_whose_data_contains_it() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    for (kind, id, parent, tags) in [
        ("requirement", "s", Value::Null, json!(["kyc", "aml"])),
        ("ac", "b", json!("requirement:r"), json!(["kyc"])),
        ("requirement", "u", Value::Null, json!(["other"])),
    ] {
        let save = json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
            "data":{"title":id,"owner":"owner","body":format!("Rule {id}"),"tags":tags},
            "reason":"Tag"});
        w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    }
    let ok = async |args: Value| read(&w, args).await?.map_err(anyhow::Error::msg);
    // Without parent the filter reads every level; with parent only the children.
    let kyc = ok(json!({"data":{"tags":["kyc"]}})).await?;
    let children = ok(json!({"data":{"tags":["kyc"]},"parent":"requirement:r"})).await?;
    let scalar = ok(json!({"data":{"body":"Rule u"},"fields":["title"]})).await?;
    let none = ok(json!({"data":{"tags":["kyc","other"]}})).await?;
    ensure!(
        addresses(&kyc) == ["ac:b", "requirement:s"]
            && kyc["total"] == 2
            && addresses(&children) == ["ac:b"]
            && scalar == json!({"items":[{"address":"requirement:u","title":"u"}],"total":1})
            && none["items"] == json!([]),
        "{kyc} {children} {scalar} {none}"
    );
    for (args, error) in [
        (json!({"data":"kyc"}), "data filter must be an object"),
        (json!({"data":["kyc"]}), "data filter must be an object"),
        (
            json!({"data":{"tags":["kyc"]},"address":"requirement:s"}),
            "address cannot be combined with list filters",
        ),
    ] {
        let reply = read(&w, args.clone()).await?;
        ensure!(reply == Err(error.into()), "{args}: {reply:?}");
    }
    Ok(())
}
