//! requirement:read-contract over a real server: an MCP search hit gives an excerpt of at most
//! 200 characters, no score, and only the fields that the request names. HTTP search keeps the
//! score and the excerpt of 600 characters.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use record_world::World;
use serde_json::{Value, json};

/// The body of 600 characters that starts with the word zebra.
fn long_body() -> String {
    format!("zebra{}", " stripe".repeat(85))
}

async fn save(w: &World, id: &str, body: &str) -> Result<()> {
    let save = json!({"op":"save_object","kind":"requirement","id":id,"revision":0,
        "parent":null,"data":{"title":id,"owner":"owner","body":body},"reason":"Create"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    Ok(())
}

/// The hit of `address` in a page; an HTTP hit names itself by kind and id.
fn hit<'a>(page: &'a Value, address: &str) -> Option<&'a Value> {
    page["items"].as_array()?.iter().find(|i| {
        let named = format!(
            "{}:{}",
            i["kind"].as_str().unwrap_or(""),
            i["id"].as_str().unwrap_or("")
        );
        i["address"] == address || named == address
    })
}

/// The keys of an object, sorted.
fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

// ac:h0wbjo1a
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_mcp_hit_gives_200_characters_no_score_and_the_named_fields() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let body = long_body();
    ensure!(body.chars().count() == 600, "the long body");
    save(&w, "long", &body).await?;
    save(&w, "short", "zebra short").await?;
    let world = &w;
    let search =
        move |args: Value| async move { world.mcp(&world.bot, "joppa_search", args).await };
    let page = search(json!({"query":"zebra"})).await?;
    let page = page.map_err(anyhow::Error::msg)?;
    let counters = ["indexed", "embedded", "mode", "semantic_status"];
    ensure!(counters.iter().all(|k| page.get(*k).is_none()), "{page}");
    let items = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(items.len() >= 2, "{page}");
    for item in &items {
        let named = ["address", "title", "revision", "excerpt"];
        ensure!(
            named.iter().all(|k| item.get(*k).is_some()) && item.get("score").is_none(),
            "{item}"
        );
    }
    let first: String = body.chars().take(200).collect();
    let long = hit(&page, "requirement:long");
    let short = hit(&page, "requirement:short");
    ensure!(
        long.is_some_and(|h| h["excerpt"] == first.as_str())
            && short.is_some_and(|h| h["excerpt"] == "zebra short"),
        "{page}"
    );
    // fields ["title","status"] gives the address, the title and the status tone of each hit.
    let named = search(json!({"query":"zebra","fields":["title","status"]})).await?;
    let named = named.map_err(anyhow::Error::msg)?;
    let items = named["items"].as_array().cloned().unwrap_or_default();
    ensure!(items.len() >= 2, "{named}");
    for item in &items {
        ensure!(
            keys(item) == ["address", "status", "title"] && item["status"].is_string(),
            "{item}"
        );
    }
    // An unknown field is refused, and the error names the fields that search takes.
    let refused = search(json!({"query":"zebra","fields":["nope"]})).await?;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains("title") && e.contains("excerpt")),
        "{refused:?}"
    );
    // HTTP keeps the score and the excerpt of 600 characters.
    let url = format!("{}/api/workspaces/{}/search?q=zebra", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.bot).send().await?;
    let http: Value = reply.error_for_status()?.json().await?;
    let long = hit(&http, "requirement:long");
    ensure!(
        long.is_some_and(|h| {
            h["score"].is_number() && h["excerpt"].as_str().map(|e| e.chars().count()) == Some(600)
        }),
        "HTTP: {http}"
    );
    Ok(())
}
