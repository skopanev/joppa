//! requirement:read-contract over a real server: a bare name in the fields of joppa_objects
//! reads the data field of that name, also when no returned document holds it.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Result, ensure};
use record_world::World;
use save_world::ok;
use serde_json::{Value, json};

async fn read(w: &World, args: Value) -> Result<Result<Value, String>> {
    w.mcp(&w.bot, "joppa_objects", args).await
}

/// The items of a list read with `fields`, by their addresses.
async fn items(w: &World, parent: &str, fields: Value) -> Result<Vec<Value>> {
    let args = json!({"parent":parent,"fields":fields});
    let page = read(w, args).await?.map_err(anyhow::Error::msg)?;
    let mut items = page["items"].as_array().cloned().unwrap_or_default();
    items.sort_by_key(|i| i["address"].as_str().unwrap_or("").to_owned());
    Ok(items)
}

/// A new criterion `id` without tags under `parent`.
fn criterion(id: &str, parent: &str) -> Value {
    json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":parent,
        "data":{"title":id.to_uppercase(),"owner":"owner"},"reason":"Create"})
}

// deploy-24 AC-8
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_bare_fields_name_reads_the_data_field_of_that_name() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tags = json!({"op":"save_object","kind":"ac","id":"a","revision":1,
        "data":{"tags":["kyc"]},"reason":"Tag"});
    ok(&w, tags).await?;
    ok(&w, criterion("b", "requirement:r")).await?;
    let q = json!({"op":"save_object","kind":"requirement","id":"q","revision":0,"parent":null,
        "data":{"title":"Q","owner":"owner","body":"Another rule"},"reason":"Create"});
    ok(&w, q).await?;
    for id in ["c", "d"] {
        ok(&w, criterion(id, "requirement:q")).await?;
    }
    for key in ["tags", "data.tags"] {
        let found = items(&w, "requirement:r", json!([key])).await?;
        let wanted = vec![
            json!({"address":"ac:a",key:["kyc"]}),
            json!({"address":"ac:b",key:null}),
        ];
        ensure!(found == wanted, "{key}: {found:?}");
    }
    let found = items(&w, "requirement:q", json!(["tags"])).await?;
    ensure!(
        found.len() == 2 && found.iter().all(|i| i.get("tags") == Some(&Value::Null)),
        "{found:?}"
    );
    let b = json!({"address":"ac:b","fields":["tags","nope"]});
    let b = read(&w, b).await?.map_err(anyhow::Error::msg)?;
    ensure!(
        b.get("tags") == Some(&Value::Null) && b.get("nope") == Some(&Value::Null),
        "{b}"
    );
    for (args, error) in [
        (
            json!({"parent":"requirement:r","fields":["comments"]}),
            "unknown field comments",
        ),
        (
            json!({"address":"ac:a","fields":["description"]}),
            "use body instead of description; read joppa_help",
        ),
    ] {
        let reply = read(&w, args.clone()).await?;
        ensure!(reply == Err(error.into()), "{args}: {reply:?}");
    }
    Ok(())
}
