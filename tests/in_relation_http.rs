//! requirement:q438uw0s over a real server: the member read in_relation gives the present nodes
//! of one relation in pages of 25, in address order, over HTTP and MCP, and takes no other filter.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{declare, msg, saved, stored};
use serde_json::{Value, json};

/// One GET of the object list with the query `query`. Gives the HTTP code and the body.
async fn get(w: &World, query: &[(&str, &str)]) -> Result<(u16, String)> {
    let url = format!("{}/api/workspaces/{}/objects", w.root, w.ws);
    let reply = w
        .http
        .get(url)
        .bearer_auth(&w.bot)
        .query(query)
        .send()
        .await?;
    Ok((reply.status().as_u16(), reply.text().await?))
}

/// The addresses of the items of a page.
fn addresses(page: &Value) -> Vec<String> {
    let items = page["items"].as_array().into_iter().flatten();
    items
        .map(|i| i["address"].as_str().unwrap_or("").to_owned())
        .collect()
}

/// A short form of a page or a refusal for an error text: the addresses, total and next_cursor.
fn brief(reply: &Result<Value, String>) -> String {
    match reply {
        Ok(page) => format!(
            "{:?} total {} next_cursor {}",
            addresses(page),
            page["total"],
            page["next_cursor"]
        ),
        Err(e) => format!("refused: {e}"),
    }
}

/// The archive of `address` by the owner.
async fn archive(w: &World, address: &str) -> Result<()> {
    let revision = stored(w, address).await?.0;
    let command =
        json!({"op":"archive_object","target":address,"revision":revision,"reason":"Done"});
    w.command(&w.owner, command).await?.map_err(msg)?;
    Ok(())
}

/// Nodes N01..N30 hold swap with an edge to the next node, N30 with an edge to Y. Z has only
/// related edges, and X holds swap. The owner archives X and Y.
async fn members() -> Result<World> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    saved(&w, "doc:y", 0, json!({})).await?;
    for i in (1..=30).rev() {
        let next = if i == 30 {
            "doc:y".to_owned()
        } else {
            format!("doc:n{:02}", i + 1)
        };
        let swap = json!({"swap":{"edges":[{"to":next}]}});
        saved(&w, &format!("doc:n{i:02}"), 0, json!({"relations":swap})).await?;
    }
    let related = json!({"related":{"edges":[{"to":"doc:n01"},{"to":"doc:n02"}]}});
    saved(&w, "doc:z", 0, json!({"relations":related})).await?;
    let swap = json!({"swap":{"edges":[{"to":"doc:n01"}]}});
    saved(&w, "doc:x", 0, json!({"relations":swap})).await?;
    archive(&w, "doc:x").await?;
    archive(&w, "doc:y").await?;
    Ok(w)
}

// ac:49128yzr
// test:wulanb57
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn in_relation_pages_the_present_members_of_a_relation_by_25() -> Result<()> {
    let w = members().await?;
    let names = |range: std::ops::RangeInclusive<u32>| -> Vec<String> {
        range.map(|i| format!("doc:n{i:02}")).collect()
    };
    let query = [
        ("in_relation", "swap"),
        ("fields", "title,signal,relations"),
        ("limit", "100"),
    ];
    let (code, text) = get(&w, &query).await?;
    ensure!(code == 200, "page 1: {code} {text}");
    let first: Value = serde_json::from_str(&text)?;
    ensure!(
        addresses(&first) == names(1..=25),
        "page 1: {}",
        brief(&Ok(first.clone()))
    );
    ensure!(
        first["total"] == 30,
        "page 1 total: {}",
        brief(&Ok(first.clone()))
    );
    let cursor = first["next_cursor"].as_str().unwrap_or("").to_owned();
    ensure!(
        !cursor.is_empty(),
        "page 1 gives next_cursor: {}",
        brief(&Ok(first.clone()))
    );
    let mut next = query.to_vec();
    next.push(("cursor", cursor.as_str()));
    let (code, text) = get(&w, &next).await?;
    ensure!(code == 200, "page 2: {code} {text}");
    let second: Value = serde_json::from_str(&text)?;
    ensure!(
        addresses(&second) == names(26..=30),
        "page 2: {}",
        brief(&Ok(second.clone()))
    );
    ensure!(
        second["total"] == 30,
        "page 2 total: {}",
        brief(&Ok(second.clone()))
    );
    ensure!(
        second.get("next_cursor").is_none_or(Value::is_null),
        "page 2 is the last page: {}",
        brief(&Ok(second.clone()))
    );
    // MCP gives the same first page. workspace and alias are allowed with in_relation.
    let mcp = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"in_relation":"swap","alias":"p1"}),
        )
        .await?
        .map_err(msg)?;
    ensure!(
        addresses(&mcp) == names(1..=25),
        "MCP: {}",
        brief(&Ok(mcp.clone()))
    );
    ensure!(
        mcp["next_cursor"] == first["next_cursor"],
        "MCP cursor: {}",
        brief(&Ok(mcp.clone()))
    );
    Ok(())
}

// ac:49128yzr
// test:89ypqn2b
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn in_relation_refuses_an_unknown_key_and_each_other_filter() -> Result<()> {
    let w = members().await?;
    let unknown = w
        .mcp(&w.bot, "joppa_objects", json!({"in_relation":"nope"}))
        .await?;
    ensure!(
        unknown
            .as_ref()
            .is_err_and(|e| e.contains("unknown link relation")),
        "MCP nope: {}",
        brief(&unknown)
    );
    let (code, text) = get(&w, &[("in_relation", "nope")]).await?;
    ensure!(
        (400..500).contains(&code) && text.contains("unknown link relation"),
        "HTTP nope: {code} {text}"
    );
    let only = "in_relation takes only fields, limit and cursor";
    for (key, value) in [
        ("parent", "*"),
        ("address", "doc:n01"),
        ("related_to", "doc:n01"),
        ("status", "green"),
    ] {
        let mut args = json!({"in_relation":"swap"});
        args[key] = json!(value);
        let reply = w.mcp(&w.bot, "joppa_objects", args).await?;
        ensure!(
            reply.as_ref().is_err_and(|e| e.contains(only)),
            "MCP {key}: {}",
            brief(&reply)
        );
        let (code, text) = get(&w, &[("in_relation", "swap"), (key, value)]).await?;
        ensure!(
            (400..500).contains(&code) && text.contains(only),
            "HTTP {key}: {code} {text}"
        );
    }
    Ok(())
}
