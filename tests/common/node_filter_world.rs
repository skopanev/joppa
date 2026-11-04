//! The fixture of the node filter tests: requirement:r > ac:a > rule:x > rule:z and
//! requirement:r > ac:b, and one read of the same filters four ways.
use super::record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// Each node of the fixture has the word "fixture" in its body, so a search finds all.
pub async fn seed(w: &record_world::World) -> Result<()> {
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let rule = json!({"title":"Rules","role":"document","status":"none","parents":["ac","rule"]});
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "put_types":{"rule":rule},"reason":"Add rules"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    // A test node under ac:a gets the same word, so a search finds it too.
    let tests = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"ac:a","kind":"test"}),
        )
        .await?;
    let tests: Vec<String> = tests.map_err(anyhow::Error::msg)?["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect();
    for address in ["requirement:r", "ac:a"]
        .into_iter()
        .map(String::from)
        .chain(tests)
    {
        let address = address.as_str();
        let object = w.object(address).await?;
        let mut data = object["data"].clone();
        data["body"] = json!(format!("Fixture text of {address}"));
        let (kind, id) = address.split_once(':').context("address")?;
        let save = json!({"op":"save_object","kind":kind,"id":id,"revision":object["revision"],
            "parent":object["parent"],"data":data,"reason":"Fixture text"});
        w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    }
    for (kind, id, parent, extra) in [
        ("ac", "b", "requirement:r", json!({"assignee":"alex"})),
        ("rule", "x", "ac:a", json!({"owner":"alex"})),
        ("rule", "z", "rule:x", json!({"tags":["x"]})),
    ] {
        let mut data = json!({"title":id,"owner":"owner","body":format!("Fixture text of {id}")});
        for (k, v) in extra.as_object().context("extra")? {
            data[k] = v.clone();
        }
        let save = json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
            "data":data,"reason":"Fixture"});
        w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

pub fn sorted(items: &Value, address: impl Fn(&Value) -> Option<String>) -> Vec<String> {
    let mut all: Vec<String> = items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(address)
        .collect();
    all.sort();
    all
}

/// The HTTP query text of MCP filter arguments: a list joins with commas.
pub fn pairs(args: &Value) -> Vec<(String, String)> {
    let pairs = args.as_object().into_iter().flatten();
    pairs
        .map(|(k, v)| {
            let text = match v {
                Value::String(s) => s.clone(),
                Value::Array(items) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
                other => other.to_string(),
            };
            (k.clone(), text)
        })
        .collect()
}

pub async fn http(
    w: &record_world::World,
    route: &str,
    pairs: &[(String, String)],
) -> Result<Value> {
    let reply = w
        .http
        .get(format!("{}/api/workspaces/{}/{route}", w.root, w.ws))
        .bearer_auth(&w.bot)
        .query(pairs)
        .send()
        .await?;
    ensure!(
        reply.status().is_success(),
        "GET {route}: {}",
        reply.text().await?
    );
    Ok(reply.json().await?)
}

/// The nodes that the filters keep, read four ways: the MCP list, the MCP search, the HTTP
/// list and the HTTP search. All four must give the same addresses.
pub async fn kept(w: &record_world::World, filters: Value) -> Result<Vec<String>> {
    let mut list_args = filters.clone();
    list_args["limit"] = json!(25);
    let list = w
        .mcp(&w.bot, "joppa_objects", list_args)
        .await?
        .map_err(anyhow::Error::msg)?;
    let mut search_args = filters.clone();
    search_args["query"] = json!("fixture");
    search_args["limit"] = json!(25);
    let search = w
        .mcp(&w.bot, "joppa_search", search_args)
        .await?
        .map_err(anyhow::Error::msg)?;
    let pairs = pairs(&filters);
    let http_list = http(w, "objects", &pairs).await?;
    let mut search_pairs = pairs.clone();
    search_pairs.push(("q".into(), "fixture".into()));
    let http_search = http(w, "search", &search_pairs).await?;
    let address = |i: &Value| i["address"].as_str().map(str::to_owned);
    let joined = |i: &Value| Some(format!("{}:{}", i["kind"].as_str()?, i["id"].as_str()?));
    let four = [
        sorted(&list["items"], address),
        sorted(&search["items"], address),
        sorted(&http_list["items"], address),
        sorted(&http_search["items"], joined),
    ];
    ensure!(
        four.iter().all(|a| *a == four[0]) && list["total"] == four[0].len(),
        "{filters}: {four:?} {list}"
    );
    Ok(four[0].clone())
}
