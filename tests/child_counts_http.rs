//! requirement:compact-open-object over a real server: each list row gives the counts of its
//! direct children by type from the status projection, and the board shows them in the row.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const ONE: &str = "tests/x.rs > one";
const TWO: &str = "tests/x.rs > two";

/// The child counts of one list row over HTTP, as the board reads them.
async fn counts(w: &record_world::World, query: &str, address: &str) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}/objects?{query}", w.root, w.ws);
    let page: Value = w
        .http
        .get(url)
        .bearer_auth(&w.alex)
        .send()
        .await?
        .json()
        .await?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let item = items.iter().find(|i| i["address"] == address);
    Ok(item.map_or(Value::Null, |i| i["child_counts"].clone()))
}

// ac:pl7zblgo
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_row_counts_its_direct_children_by_type_and_their_green_ones() -> Result<()> {
    let w = record_world::world(&[ONE, TWO]).await?;
    for (id, title) in [("b", "B"), ("c", "C")] {
        let save = json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:r",
            "data":{"title":title,"owner":"owner"},"reason":"Create"});
        w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    }
    let archive = json!({"op":"archive_object","target":"ac:c","revision":1,"reason":"Not needed"});
    w.command(&w.owner, archive)
        .await?
        .map_err(anyhow::Error::msg)?;
    // No result yet: two tests of A count, and none of them is green.
    let roots = counts(&w, "", "requirement:r").await?;
    ensure!(
        roots == json!([{"kind":"ac","total":2,"passed":0}]),
        "{roots}"
    );
    let tests = counts(&w, "parent=requirement:r&depth=1", "ac:a").await?;
    ensure!(
        tests == json!([{"kind":"test","total":2,"passed":0}]),
        "{tests}"
    );
    ensure!(
        counts(&w, "parent=requirement:r&depth=1", "ac:b")
            .await?
            .is_null()
    );
    // One pass makes one test green, and two passes make A green for R.
    let pass = |name: &str| json!({"test":name,"status":"pass"});
    w.recorded(
        "1111111111111111111111111111111111111111",
        json!([pass(ONE)]),
    )
    .await?;
    let tests = counts(&w, "parent=requirement:r&depth=1", "ac:a").await?;
    ensure!(
        tests == json!([{"kind":"test","total":2,"passed":1}]),
        "{tests}"
    );
    w.recorded(
        "2222222222222222222222222222222222222222",
        json!([pass(ONE), pass(TWO)]),
    )
    .await?;
    let roots = counts(&w, "", "requirement:r").await?;
    ensure!(
        roots == json!([{"kind":"ac","total":2,"passed":1}]),
        "{roots}"
    );
    let tests = counts(&w, "parent=requirement:r&depth=1", "ac:a").await?;
    ensure!(
        tests == json!([{"kind":"test","total":2,"passed":2}]),
        "{tests}"
    );
    // An MCP list gives the counts when fields names them.
    let args = json!({"parent":"requirement:r","fields":["child_counts"]});
    let list = w.mcp(&w.bot, "joppa_objects", args).await?;
    let list = list.map_err(anyhow::Error::msg)?;
    let a = list["items"].as_array().into_iter().flatten();
    let a: Vec<&Value> = a.filter(|i| i["address"] == "ac:a").collect();
    ensure!(
        a.len() == 1 && a[0]["child_counts"] == json!([{"kind":"test","total":2,"passed":2}]),
        "{list}"
    );
    Ok(())
}
