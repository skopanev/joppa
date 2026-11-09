//! requirement:grouped-links over a real server: an object read gives edge counts by group
//! (relation_groups) and no incoming list, and a relations_of read gives the edges of one
//! group in pages over HTTP and MCP.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn db() -> Result<(deadpool_postgres::Pool, String)> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok((pool, url))
}

fn save(kind: &str, id: &str, parent: Option<&str>, relations: Value) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":format!("{kind} {id}"),"owner":"owner"},"relations":relations,"reason":"Link group test"})
}

async fn http_page(root: &str, ws: &str, cursor: Option<&str>) -> Result<Value> {
    let mut query = vec![
        ("relations_of", "domain:r"),
        ("direction", "incoming"),
        ("relation", "related"),
        ("kind", "note"),
        ("limit", "25"),
    ];
    if let Some(cursor) = cursor {
        query.push(("cursor", cursor));
    }
    Ok(reqwest::Client::new()
        .get(format!("{root}/api/workspaces/{ws}/objects"))
        .bearer_auth(world::token("bot"))
        .query(&query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

// grouped-links-read
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_object_read_gives_link_counts_and_a_group_reads_in_pages() -> Result<()> {
    let (pool, url) = db().await?;
    let ws = format!("links-{}", uuid::Uuid::new_v4().simple());
    let write = |command: Value| {
        let (pool, ws) = (pool.clone(), ws.clone());
        async move {
            store::execute_input(
                &pool,
                &ws,
                "owner",
                &uuid::Uuid::new_v4().to_string(),
                &command,
            )
            .await
        }
    };
    write(json!({"op":"workspace","title":"Links"})).await?;
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    write(save("domain", "home", None, json!({}))).await?;
    for i in 0..3 {
        write(save(
            "capability",
            &format!("c{i}"),
            Some("domain:home"),
            json!({}),
        ))
        .await?;
    }
    // Three outgoing depends_on edges. A depends_on edge always blocks.
    let targets: Vec<Value> = (0..3)
        .map(|i| json!({"to":format!("capability:c{i}")}))
        .collect();
    write(save(
        "domain",
        "r",
        None,
        json!({"depends_on":{"edges":targets}}),
    ))
    .await?;
    for i in 0..120 {
        write(save(
            "note",
            &format!("n{i:03}"),
            None,
            json!({"related":{"edges":[{"to":"domain:r"}]}}),
        ))
        .await?;
    }
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    let object = world::mcp(
        &server.root,
        "bot",
        "joppa_objects",
        json!({"workspace":ws,"address":"domain:r"}),
    )
    .await?;
    ensure!(
        object.get("incoming").is_none(),
        "the read still lists incoming edges"
    );
    let groups = object["relation_groups"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(
        groups.contains(
            &json!({"direction":"incoming","relation":"related","kind":"note","count":120})
        ),
        "{groups:?}"
    );
    // `relations` gives the outgoing edges, so the counts keep only the incoming groups.
    let depends_on = object["relations"]["depends_on"]["edges"]
        .as_array()
        .map(Vec::len);
    ensure!(
        groups.iter().all(|g| g["direction"] == "incoming")
            && depends_on == Some(3)
            && object.get("links").is_none()
            && object.get("link_groups").is_none(),
        "{groups:?} {}",
        object["relations"]
    );
    let (mut cursor, mut seen, mut sizes) =
        (None::<String>, std::collections::BTreeSet::new(), vec![]);
    loop {
        let mut args = json!({"workspace":ws,"relations_of":"domain:r","direction":"incoming","relation":"related","kind":"note","limit":25});
        if let Some(c) = &cursor {
            args["cursor"] = json!(c);
        }
        let tool = world::mcp(&server.root, "bot", "joppa_objects", args).await?;
        // MCP and HTTP give the same page: the same items, the same total and the same next
        // cursor. Each item names its relation and gives no blocking.
        let http = http_page(&server.root, &ws, cursor.as_deref()).await?;
        let addresses = |page: &Value| -> Vec<Value> {
            let items = page["items"].as_array().cloned().unwrap_or_default();
            items.iter().map(|i| i["address"].clone()).collect()
        };
        ensure!(
            addresses(&tool) == addresses(&http)
                && tool["total"] == http["total"]
                && tool["next_cursor"] == http["next_cursor"],
            "HTTP and MCP disagree:\n{tool}\n{http}"
        );
        let items = tool["items"].as_array().cloned().unwrap_or_default();
        ensure!(
            items
                .iter()
                .all(|i| i["relation"] == "related" && i.get("blocking").is_none()),
            "{tool}"
        );
        sizes.push(items.len());
        for item in &items {
            ensure!(
                seen.insert(item["address"].as_str().unwrap_or("").to_owned()),
                "an edge came twice: {item}"
            );
        }
        match tool["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    ensure!(
        sizes == [25, 25, 25, 25, 20] && seen.len() == 120,
        "pages {sizes:?}, edges {}",
        seen.len()
    );
    // Each filter is optional: without them the read gives every edge of both directions,
    // and each item names its direction, relation and kind.
    let (mut cursor, mut all) = (None::<String>, vec![]);
    loop {
        let mut args = json!({"workspace":ws,"relations_of":"domain:r","limit":100});
        if let Some(c) = &cursor {
            args["cursor"] = json!(c);
        }
        let page = world::mcp(&server.root, "bot", "joppa_objects", args).await?;
        ensure!(page["total"] == 123, "{page}");
        all.extend(page["items"].as_array().cloned().unwrap_or_default());
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    // An unfiltered item names its direction, its relation and its kind.
    let count = |direction: &str, relation: &str| {
        all.iter()
            .filter(|i| {
                i["direction"] == direction && i["relation"] == relation && i["kind"].is_string()
            })
            .count()
    };
    ensure!(
        all.len() == 123
            && count("incoming", "related") == 120
            && count("outgoing", "depends_on") == 3,
        "{all:?}"
    );
    let notes = world::mcp(
        &server.root,
        "bot",
        "joppa_objects",
        json!({"workspace":ws,"relations_of":"domain:r","kind":"note","limit":1}),
    )
    .await?;
    ensure!(
        notes["total"] == 120 && notes["items"][0]["direction"] == "incoming",
        "{notes}"
    );
    Ok(())
}
