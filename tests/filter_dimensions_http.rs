//! requirement:lfctzswy over a real server: a filter field that a result field also
//! names is a dimension. A data filter with its value keeps the nodes that apply to the value
//! and gives the tones of the results that count for it.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use record_world::World;
use serde_json::{Value, json};

async fn ok(w: &World, token: &str, command: Value) -> Result<Value> {
    w.command(token, command).await?.map_err(anyhow::Error::msg)
}

/// The address and the status of each node below requirement:r for `data`.
async fn tones(w: &World, data: Value) -> Result<Vec<(String, String)>> {
    let args =
        json!({"parent":"requirement:r","depth":100,"data":data,"fields":["status","title"]});
    let page = w
        .mcp(&w.bot, "joppa_objects", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let mut found: Vec<(String, String)> = items
        .iter()
        .map(|i| {
            let name = i["title"].as_str().unwrap_or_default().to_owned();
            (name, i["status"].as_str().unwrap_or_default().to_owned())
        })
        .collect();
    found.sort();
    Ok(found)
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

// ac:gdmz97e6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_dimension_value_keeps_the_nodes_that_apply_and_gives_their_tones() -> Result<()> {
    let (t1, t2) = ("tests/x.rs > t1", "tests/x.rs > t2");
    let w = record_world::world(&[t1]).await?;
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let mut rules = tree["schema"]["test_results"].clone();
    rules["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"env","type":"enum","values":["dev","prod"]}));
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"filters":["env"],
        "test_results":rules,"reason":"Colour the tests by environment"});
    ok(&w, &w.owner, schema).await?;
    // ac:b applies to prod only, and its test node t2 inherits the value.
    let b = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
        "data":{"title":"B","owner":"owner","env":"prod"},"reason":"Prod only"});
    ok(&w, &w.bot, b).await?;
    let t = json!({"op":"save_object","kind":"test","revision":0,"parent":"ac:b","data":{"title":t2,"owner":"owner"},"reason":"Bind"});
    ok(&w, &w.bot, t).await?;
    // t1 passes on dev and fails later on prod; t2 has no env, so it counts for each value.
    w.recorded(
        &"a".repeat(40),
        json!([{"test":t1,"status":"pass","env":"dev"},{"test":t2,"status":"pass"}]),
    )
    .await?;
    w.recorded(
        &"b".repeat(40),
        json!([{"test":t1,"status":"fail","env":"prod"}]),
    )
    .await?;
    let base = tones(&w, json!({})).await?;
    ensure!(
        base == pairs(&[("A", "red"), ("B", "green"), (t1, "red"), (t2, "green")]),
        "base {base:?}"
    );
    let dev = tones(&w, json!({"env":"dev"})).await?;
    ensure!(
        dev == pairs(&[("A", "green"), (t1, "green")]),
        "dev {dev:?}"
    );
    let prod = tones(&w, json!({"env":["prod"]})).await?;
    ensure!(
        prod == pairs(&[("A", "red"), ("B", "green"), (t1, "red"), (t2, "green")]),
        "prod {prod:?}"
    );
    // The requirement takes the tones of the criteria that apply.
    let r = |data: Value| {
        let args = json!({"data":data,"kind":"requirement","fields":["status"]});
        w.mcp(&w.bot, "joppa_objects", args)
    };
    let dev_r = r(json!({"env":"dev"})).await?.map_err(anyhow::Error::msg)?;
    ensure!(dev_r["items"][0]["status"] == "green", "{dev_r}");
    let prod_r = r(json!({"env":"prod"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(prod_r["items"][0]["status"] == "red", "{prod_r}");
    // The status filter reads the tone of the combination.
    let args = json!({"parent":"*","data":{"env":"dev"},"status":"green","kind":"ac"});
    let green = w
        .mcp(&w.bot, "joppa_objects", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        green["total"] == 1 && green["items"][0]["address"] == "ac:a",
        "{green}"
    );
    // The board offers each value of the dimension.
    let url = format!("{}/api/workspaces/{}/board", w.root, w.ws);
    let board: Value = w
        .http
        .get(url)
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    let wanted = json!([{"field":"env","values":[{"value":"dev","list":false},{"value":"prod","list":false}]}]);
    ensure!(board["filters"] == wanted, "{}", board["filters"]);
    ensure!(
        board["dimensions"] == json!([{"field":"env","values":["dev","prod"]}]),
        "{}",
        board["dimensions"]
    );
    Ok(())
}
