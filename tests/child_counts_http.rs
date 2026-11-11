//! requirement:compact-open-object over a real server: each list row gives the counts of its
//! direct children by type from the status projection, and the board shows them in the row.
//! An MCP detail gives child_counts and references only when fields names them.
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

/// A write with `token` that must succeed.
async fn write(w: &record_world::World, token: &str, command: Value) -> Result<()> {
    w.command(token, command)
        .await?
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// A consumer type, a note type and the consumer reference of ac.
async fn configure(w: &record_world::World) -> Result<()> {
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    let mut types = tree["schema"]["types"].clone();
    types["consumer"] = json!({"title":"Consumers","role":"document","status":"none",
        "root":true,"parents":[]});
    types["note"] = json!({"title":"Notes","role":"document","inbox":true,"status":"none",
        "root":true,"parents":[]});
    types["ac"]["references"] =
        json!({"consumer":{"target":"consumer","title":"Used by","multiple":false}});
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "types":types,"reason":"Consumers and notes"});
    write(w, &w.owner, schema).await
}

// ac:r9gwjikf
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_detail_gives_child_counts_and_references_only_with_fields() -> Result<()> {
    let w = record_world::world(&[ONE, TWO]).await?;
    configure(&w).await?;
    write(
        &w,
        &w.bot,
        json!({"op":"save_object","kind":"consumer","id":"agent","revision":0,
        "parent":null,"data":{"title":"Agent","owner":"owner"},"reason":"Create"}),
    )
    .await?;
    write(
        &w,
        &w.bot,
        json!({"op":"save_object","kind":"ac","id":"a","revision":1,
        "data":{"consumer":"consumer:agent"},"reason":"Name the consumer"}),
    )
    .await?;
    write(
        &w,
        &w.bot,
        json!({"op":"save_object","kind":"note","id":"n","revision":0,
        "parent":null,"data":{"title":"N","owner":"owner","body":"Input"},
        "relations":{"related":{"edges":[{"to":"ac:a"}]}},"reason":"Create"}),
    )
    .await?;
    w.recorded(
        "3333333333333333333333333333333333333333",
        json!([{"test":ONE,"status":"pass"}]),
    )
    .await?;
    let world = &w;
    let read = |args: Value| async move { world.mcp(&world.bot, "joppa_objects", args).await };
    let detail = read(json!({"address":"ac:a"})).await?;
    let detail = detail.map_err(anyhow::Error::msg)?;
    ensure!(
        detail.get("child_counts").is_none()
            && detail.get("references").is_none()
            && detail["relation_groups"] == json!([{"relation":"related","kind":"note","count":1}]),
        "{detail}"
    );
    let named = json!({"address":"ac:a","fields":["child_counts","references"]});
    let named = read(named).await?.map_err(anyhow::Error::msg)?;
    let list = json!({"parent":"requirement:r","fields":["child_counts"]});
    let list = read(list).await?.map_err(anyhow::Error::msg)?;
    let item = list["items"].as_array().into_iter().flatten();
    let item: Vec<&Value> = item.filter(|i| i["address"] == "ac:a").collect();
    let counts = json!([{"kind":"test","total":2,"passed":1}]);
    ensure!(
        named["child_counts"] == counts
            && item.len() == 1
            && item[0]["child_counts"] == named["child_counts"],
        "{named} {list}"
    );
    let references = named["references"].as_array().cloned().unwrap_or_default();
    ensure!(
        references.len() == 1
            && references[0]["target"] == "consumer:agent"
            && references[0]["field"] == "consumer"
            && references[0]["title"] == "Agent"
            && references[0]["current_revision"] == 1,
        "{named}"
    );
    // The HTTP read keeps child_counts, references and the groups with direction.
    let url = format!("{}/api/workspaces/{}/objects?address=ac:a", w.root, w.ws);
    let http: Value = w
        .http
        .get(url)
        .bearer_auth(&w.alex)
        .send()
        .await?
        .json()
        .await?;
    let group = json!({"direction":"incoming","relation":"related","kind":"note","count":1});
    ensure!(
        http["child_counts"] == counts
            && http["references"]
                .as_array()
                .is_some_and(|r| r.len() == 1 && r[0]["target"] == "consumer:agent")
            && http["relation_groups"]
                .as_array()
                .is_some_and(|g| g.contains(&group)),
        "{http}"
    );
    Ok(())
}
