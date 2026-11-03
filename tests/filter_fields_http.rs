//! requirement:filter-fields over a real server: the tree schema names the data fields of the
//! board filter drop-downs, and the board read gives the present values of each field.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use record_world::World;
use serde_json::{Value, json};

async fn ok(w: &World, token: &str, command: Value) -> Result<Value> {
    w.command(token, command).await?.map_err(anyhow::Error::msg)
}

async fn board(w: &World) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}/board", w.root, w.ws);
    Ok(w.http
        .get(url)
        .bearer_auth(&w.bot)
        .send()
        .await?
        .json()
        .await?)
}

async fn revision(w: &World) -> Result<Value> {
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    Ok(tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone())
}

// filter-fields-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_schema_names_the_filter_fields_and_the_board_gives_their_values() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let set = json!({"op":"set_tree_schema","revision":revision(&w).await?,"filters":["tags","priority"],"reason":"Offer two filters"});
    ok(&w, &w.owner, set).await?;
    let node = |id: &str, extra: Value| {
        let mut data = json!({"title":id,"owner":"owner","body":"A rule"});
        data.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"op":"save_object","kind":"requirement","id":id,"revision":0,"parent":null,"data":data,"reason":"Add"})
    };
    ok(
        &w,
        &w.bot,
        node("s", json!({"tags":["swap","fx"],"priority":"p0"})),
    )
    .await?;
    ok(&w, &w.bot, node("t", json!({"tags":["swap"]}))).await?;
    ok(&w, &w.bot, node("old", json!({"tags":["gone"]}))).await?;
    let archive =
        json!({"op":"archive_object","target":"requirement:old","revision":1,"reason":"Old"});
    ok(&w, &w.bot, archive).await?;
    let wanted = json!([
        {"field":"tags","values":[{"value":"fx","list":true},{"value":"swap","list":true}]},
        {"field":"priority","values":[{"value":"p0","list":false}]}
    ]);
    let read = board(&w).await?;
    ensure!(read["filters"] == wanted, "{}", read["filters"]);
    let tree = w.mcp(&w.bot, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    ensure!(
        tree["schema"]["filters"] == json!(["tags", "priority"]),
        "{tree}"
    );
    // A choice filters with the data node filter.
    let list = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"*","data":{"tags":["swap"]}}),
        )
        .await?;
    let list = list.map_err(anyhow::Error::msg)?;
    let found: Vec<&Value> = list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| &i["address"])
        .collect();
    ensure!(
        found == [&json!("requirement:s"), &json!("requirement:t")],
        "{list}"
    );
    // A change without filters keeps the list. A bad list is refused and changes nothing.
    let keep = json!({"op":"set_tree_schema","revision":revision(&w).await?,"priorities":["p0","p1"],"reason":"Two levels"});
    ok(&w, &w.owner, keep).await?;
    ensure!(board(&w).await?["filters"] == wanted, "the list stays");
    for bad in [json!(["tags", "tags"]), json!(["Tags"]), json!(["body"])] {
        let change = json!({"op":"set_tree_schema","revision":revision(&w).await?,"filters":bad,"reason":"Bad"});
        let refused = w.command(&w.owner, change).await?;
        ensure!(
            refused == Err(joppa::filter_fields::REFUSAL.into()),
            "{refused:?}"
        );
    }
    let none = json!({"op":"set_tree_schema","revision":revision(&w).await?,"filters":[],"reason":"No filters"});
    ok(&w, &w.owner, none).await?;
    ensure!(
        board(&w).await?["filters"] == json!([]),
        "an empty list offers no filter"
    );
    Ok(())
}
