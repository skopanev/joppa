//! requirement:grouped-links over a real server: an object read gives link counts by group
//! and no incoming list, and a group read gives the links of one group in pages over HTTP
//! and MCP.
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

fn save(kind: &str, id: &str, parent: Option<&str>, links: Value) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":format!("{kind} {id}"),"owner":"owner"},"links":links,"reason":"Link group test"})
}

async fn http_page(root: &str, ws: &str, cursor: Option<&str>) -> Result<Value> {
    let mut query = vec![
        ("links_of", "domain:r"),
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
    write(save("domain", "home", None, json!([]))).await?;
    for i in 0..3 {
        write(save(
            "capability",
            &format!("c{i}"),
            Some("domain:home"),
            json!([]),
        ))
        .await?;
    }
    let targets: Vec<Value> = (0..3)
        .map(|i| json!({"target":format!("capability:c{i}"),"relation":"depends_on"}))
        .collect();
    write(save("domain", "r", None, json!(targets))).await?;
    for i in 0..120 {
        write(save(
            "note",
            &format!("n{i:03}"),
            None,
            json!([{"target":"domain:r","relation":"related"}]),
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
        "the read still lists incoming links"
    );
    let groups = object["link_groups"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(
        groups.contains(
            &json!({"direction":"incoming","relation":"related","kind":"note","count":120})
        ),
        "{groups:?}"
    );
    // `links` gives the outgoing links, so the counts keep only the incoming groups.
    ensure!(
        groups.iter().all(|g| g["direction"] == "incoming")
            && object["links"]["total"] == 3
            && object["links"]["items"]
                .as_array()
                .into_iter()
                .flatten()
                .all(|l| l["relation"] == "depends_on"),
        "{groups:?} {}",
        object["links"]
    );
    let (mut cursor, mut seen, mut sizes) =
        (None::<String>, std::collections::BTreeSet::new(), vec![]);
    loop {
        let mut args = json!({"workspace":ws,"links_of":"domain:r","direction":"incoming","relation":"related","kind":"note","limit":25});
        if let Some(c) = &cursor {
            args["cursor"] = json!(c);
        }
        let tool = world::mcp(&server.root, "bot", "joppa_objects", args).await?;
        // MCP gives the HTTP page without empty keys and without the echo of the filters:
        // an item names no direction, relation or kind that the request sent.
        let mut http = joppa::read_compact::strip_reply(
            http_page(&server.root, &ws, cursor.as_deref()).await?,
        );
        for key in ["direction", "relation", "kind"] {
            http.as_object_mut().map(|m| m.remove(key));
            for item in http["items"].as_array_mut().into_iter().flatten() {
                item.as_object_mut().map(|m| m.remove(key));
            }
        }
        ensure!(tool == http, "HTTP and MCP disagree:\n{tool}\n{http}");
        ensure!(
            tool["workspace"] == ws.as_str() && tool["position"].is_u64(),
            "{tool}"
        );
        let items = tool["items"].as_array().cloned().unwrap_or_default();
        sizes.push(items.len());
        for item in &items {
            ensure!(
                seen.insert(item["address"].as_str().unwrap_or("").to_owned()),
                "a link came twice: {item}"
            );
        }
        match tool["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    ensure!(
        sizes == [25, 25, 25, 25, 20] && seen.len() == 120,
        "pages {sizes:?}, links {}",
        seen.len()
    );
    // Each filter is optional: without them the read gives every link of both directions,
    // and each item names its direction, relation and kind.
    let (mut cursor, mut all) = (None::<String>, vec![]);
    loop {
        let mut args = json!({"workspace":ws,"links_of":"domain:r","limit":100});
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
        json!({"workspace":ws,"links_of":"domain:r","kind":"note","limit":1}),
    )
    .await?;
    ensure!(
        notes["total"] == 120
            && notes["items"][0].get("kind").is_none()
            && notes["items"][0]["direction"] == "incoming",
        "{notes}"
    );
    Ok(())
}
