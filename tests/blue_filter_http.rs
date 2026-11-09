//! requirement:vqlbur46 over a real server: the status filter takes blue and the older name
//! grey as one tone in joppa_objects, joppa_search and their HTTP routes. The fixture of the
//! node filter tests adds criterion E with a failed test and the dimension field stage.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/node_filter_world.rs"]
mod node_filter_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use blue_world::{T, test_node};
use node_filter_world::{http, kept, pairs, seed, sorted};
use record_world::World;
use serde_json::{Value, json};

const E: &str = "tests/blue.rs > e";

async fn ok(w: &World, token: &str, command: Value) -> Result<Value> {
    w.command(token, command).await?.map_err(anyhow::Error::msg)
}

/// Criterion E with a failed test, and the dimension field stage with T at stage done.
async fn fixture(w: &World) -> Result<String> {
    seed(w).await?;
    let e = json!({"op":"save_object","kind":"ac","id":"e","revision":0,"parent":"requirement:r",
        "data":{"title":"E","owner":"owner","body":"Fixture text of e"},"reason":"Fixture"});
    ok(w, &w.bot, e).await?;
    let test = json!({"op":"save_object","kind":"test","revision":0,"parent":"ac:e",
        "data":{"title":E,"owner":"owner","body":"Fixture text of the test of e"},"reason":"Bind"});
    ok(w, &w.bot, test).await?;
    w.recorded(&"e".repeat(40), json!([{"test":E,"status":"fail"}]))
        .await?;
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    let mut rules = tree["schema"]["test_results"].clone();
    if let Some(fields) = rules["fields"].as_array_mut() {
        fields.push(json!({"name":"stage","type":"enum","values":["draft","done"]}));
    }
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "filters":["stage"],"test_results":rules,"reason":"Filter by stage"});
    ok(w, &w.owner, schema).await?;
    let t = test_node(w).await?;
    let object = w.object(&t).await?;
    let (kind, id) = t.split_once(':').unwrap_or_default();
    let stage = json!({"op":"save_object","kind":kind,"id":id,"revision":object["revision"],
        "data":{"stage":"done"},"reason":"T is done"});
    ok(w, &w.bot, stage).await?;
    Ok(t)
}

/// The filters below requirement:r with the keys of `extra`.
fn under(extra: Value) -> Value {
    let mut filters = json!({"parent":"requirement:r","depth":100});
    for (k, v) in extra.as_object().into_iter().flatten() {
        filters[k] = v.clone();
    }
    filters
}

/// The addresses of a joppa_objects list and of GET /objects with the same filters.
async fn both_lists(w: &World, filters: Value) -> Result<[Vec<String>; 2]> {
    let address = |i: &Value| i["address"].as_str().map(str::to_owned);
    let mut args = filters.clone();
    args["limit"] = json!(25);
    let list = w.mcp(&w.bot, "joppa_objects", args).await?;
    let list = list.map_err(anyhow::Error::msg)?;
    let page = http(w, "objects", &pairs(&filters)).await?;
    Ok([
        sorted(&list["items"], address),
        sorted(&page["items"], address),
    ])
}

// ac:nvghv6nt
// test:shy3nx2h
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_status_filter_takes_blue_and_grey_as_one_tone() -> Result<()> {
    let w = record_world::world(&[T]).await?;
    let t = fixture(&w).await?;
    // Each read of kept: joppa_objects, joppa_search, GET /objects and GET /search.
    let blue = kept(&w, under(json!({"status":"blue"}))).await?;
    let grey = kept(&w, under(json!({"status":"grey"}))).await?;
    ensure!(blue == grey, "blue {blue:?}, grey {grey:?}");
    ensure!(
        blue.contains(&"ac:a".to_owned()) && blue.contains(&t) && !blue.contains(&"ac:e".into()),
        "blue {blue:?}"
    );
    let blue_red = kept(&w, under(json!({"status":["blue","red"]}))).await?;
    let grey_red = kept(&w, under(json!({"status":["grey","red"]}))).await?;
    ensure!(blue_red == grey_red, "{blue_red:?} {grey_red:?}");
    ensure!(
        blue_red.contains(&"ac:e".into()) && blue.iter().all(|a| blue_red.contains(a)),
        "blue and red {blue_red:?}"
    );
    // The status and a dimension value together.
    let done = json!({"stage":"done"});
    let blue = both_lists(&w, under(json!({"status":"blue","data":done}))).await?;
    let grey = both_lists(&w, under(json!({"status":"grey","data":done}))).await?;
    ensure!(
        blue == grey && blue[0] == blue[1],
        "blue {blue:?}, grey {grey:?}"
    );
    ensure!(blue[0].contains(&t), "T is at stage done: {blue:?}");
    Ok(())
}

// ac:nvghv6nt
// test:k5ejyfqb
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn gray_is_refused_and_the_tool_texts_name_blue() -> Result<()> {
    let w = record_world::world(&[T]).await?;
    let reply = w
        .mcp(&w.bot, "joppa_objects", json!({"status":"gray"}))
        .await?;
    let error = "unknown status gray; use red, purple, yellow, orange, blue, green";
    ensure!(reply == Err(error.into()), "{reply:?}");
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let reply: Value = (w.http.post(format!("{}/mcp", w.root)))
        .bearer_auth(&w.bot)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tools = reply["result"]["tools"].as_array().cloned();
    let mut texts = Vec::new();
    for tool in tools.into_iter().flatten() {
        let status = &tool["inputSchema"]["properties"]["status"];
        if let Some(text) = status["description"].as_str() {
            texts.push((tool["name"].clone(), text.to_owned()));
        }
    }
    ensure!(!texts.is_empty(), "no status filter in tools/list: {reply}");
    for (name, text) in &texts {
        let lower = text.to_lowercase();
        ensure!(
            lower.contains("blue") && !lower.contains("grey"),
            "{name}: {text}"
        );
    }
    Ok(())
}
