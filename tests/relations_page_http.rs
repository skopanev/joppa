//! requirement:q438uw0s over a real server: relations_of pages the edges of a node by 25 with
//! a stable cursor over MCP and HTTP.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{change, declare, msg};
use serde_json::{Value, json};

/// The related targets n10..n39 of A.
fn related(extra: &[&str]) -> Value {
    let mut edges: Vec<Value> = (10..40)
        .map(|i| json!({"to":format!("doc:n{i}")}))
        .collect();
    edges.extend(extra.iter().map(|t| json!({"to":t})));
    json!(edges)
}

/// One save of the bot through the store.
async fn write(w: &World, address: &str, parent: Value, relations: Value) -> Result<()> {
    let (kind, id) = address.split_once(':').unwrap_or_default();
    let command = json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
        "data":{"title":id,"owner":"owner"},"relations":relations,"reason":"Edge fixture"});
    let request = uuid::Uuid::new_v4().to_string();
    joppa::store::execute_remote_input(&w.pool, &w.ws, "bot", &request, &command).await?;
    Ok(())
}

/// One relations_of page over MCP and over HTTP GET. Both give the same items and totals.
async fn page(w: &World, filter: Value, cursor: Option<&str>) -> Result<Value> {
    let mut args = json!({"relations_of":"doc:a","limit":25});
    for (k, v) in filter.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    if let Some(cursor) = cursor {
        args["cursor"] = json!(cursor);
    }
    let mcp = w
        .mcp(&w.bot, "joppa_objects", args.clone())
        .await?
        .map_err(msg)?;
    let query: Vec<(String, String)> = args
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), v.as_str().map_or(v.to_string(), str::to_owned)))
        .collect();
    let url = format!("{}/api/workspaces/{}/objects", w.root, w.ws);
    let reply = w
        .http
        .get(url)
        .bearer_auth(&w.bot)
        .query(&query)
        .send()
        .await?;
    let http: Value = reply.error_for_status()?.json().await?;
    // MCP may leave out the key of a filter, such as direction, but never relation.
    let keys = |v: &Value| -> Vec<(Value, Value)> {
        let items = v["items"].as_array().into_iter().flatten();
        items
            .map(|i| (i["relation"].clone(), i["address"].clone()))
            .collect()
    };
    ensure!(
        keys(&mcp) == keys(&http) && mcp["total"] == http["total"],
        "{mcp}\n{http}"
    );
    for (name, reply) in [("MCP", &mcp), ("HTTP", &http)] {
        for item in reply["items"].as_array().into_iter().flatten() {
            let mut required = vec!["address", "relation"];
            if name == "HTTP" || filter == json!({}) {
                required.extend(["title", "kind", "direction"]);
            }
            if name == "HTTP" {
                required.push("present");
            }
            let allowed = [
                "address",
                "title",
                "kind",
                "relation",
                "direction",
                "when",
                "revision",
                "present",
            ];
            let map = item.as_object().cloned().unwrap_or_default();
            ensure!(
                required.iter().all(|k| map.contains_key(*k)),
                "{name}: {item}"
            );
            ensure!(
                map.keys().all(|k| allowed.contains(&k.as_str())),
                "{name}: {item}"
            );
        }
    }
    Ok(http)
}

/// The (direction, relation, address) of each item of a page.
fn rows(page: &Value) -> Vec<(String, String, String)> {
    let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
    let items = page["items"].as_array().into_iter().flatten();
    items
        .map(|i| {
            (
                text(&i["direction"]),
                text(&i["relation"]),
                text(&i["address"]),
            )
        })
        .collect()
}

// ac:hed5l9dd
// test:8195px9w
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn relations_of_pages_the_edges_by_25_with_a_stable_cursor() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    for i in 0..40 {
        write(&w, &format!("doc:n{i:02}"), Value::Null, json!({})).await?;
    }
    for address in ["doc:s1", "doc:s2"] {
        write(&w, address, Value::Null, json!({})).await?;
    }
    write(&w, "ac:s3", json!("requirement:r"), json!({})).await?;
    let swap = json!({"edges":[{"to":"doc:s1"},{"to":"doc:s2"},{"to":"ac:s3"}]});
    let edges = json!({"related":{"edges":related(&[])},"swap":swap});
    write(&w, "doc:a", Value::Null, edges).await?;
    let depends = json!({"depends_on":{"edges":[{"to":"doc:a"}]}});
    for address in ["ac:i1", "ac:i2"] {
        write(&w, address, json!("requirement:r"), depends.clone()).await?;
    }
    let first = page(&w, json!({}), None).await?;
    let first_rows = rows(&first);
    ensure!(
        first_rows.len() == 25 && first["total"] == 35,
        "page 1: {first}"
    );
    let cursor = first["next_cursor"].as_str().unwrap_or("").to_owned();
    ensure!(!cursor.is_empty(), "page 1 gives a cursor: {first}");
    // Between the pages, A gets a related edge to n00, which sorts before the cursor.
    change(
        &w,
        "doc:a",
        json!({"relations":{"related":{"edges":related(&["doc:n00"])}}}),
    )
    .await?;
    let second = page(&w, json!({}), Some(&cursor)).await?;
    let second_rows = rows(&second);
    ensure!(
        second_rows.len() == 10 && second["total"] == 36 && second["next_cursor"].is_null(),
        "page 2: {second}"
    );
    let all: Vec<_> = first_rows.iter().chain(&second_rows).cloned().collect();
    let unique: std::collections::BTreeSet<_> = all.iter().collect();
    ensure!(unique.len() == 35, "no item repeats: {all:?}");
    // The order is direction, relation, address, and n00 is not on page 2.
    let mut grouped = all.clone();
    grouped.sort_by(|a, b| (a.0 != all[0].0, &a.1, &a.2).cmp(&(b.0 != all[0].0, &b.1, &b.2)));
    ensure!(grouped == all, "order: {all:?}");
    ensure!(!second_rows.iter().any(|r| r.2 == "doc:n00"), "{second}");
    let swap = page(&w, json!({"relation":"swap"}), None).await?;
    ensure!(
        rows(&swap).len() == 3 && rows(&swap).iter().all(|r| r.1 == "swap"),
        "{swap}"
    );
    let incoming = page(&w, json!({"direction":"incoming"}), None).await?;
    let found: Vec<_> = rows(&incoming).into_iter().map(|r| (r.1, r.2)).collect();
    let wanted = [("depends_on", "ac:i1"), ("depends_on", "ac:i2")]
        .map(|(r, a)| (r.to_owned(), a.to_owned()));
    ensure!(found == wanted, "{incoming}");
    let ac = page(&w, json!({"kind":"ac"}), None).await?;
    let mut found: Vec<String> = rows(&ac).into_iter().map(|r| r.2).collect();
    found.sort();
    let kinds = ac["items"]
        .as_array()
        .into_iter()
        .flatten()
        .all(|i| i["kind"] == "ac");
    ensure!(kinds && found == ["ac:i1", "ac:i2", "ac:s3"], "{ac}");
    Ok(())
}
