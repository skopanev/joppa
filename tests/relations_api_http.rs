//! requirement:q438uw0s over a real server: the live API reads and writes relations, refuses
//! links and links_of, and takes a public item ID as an edge target.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{change, edges, last_event, msg, relations, saved, to, without_key};
use serde_json::{Value, json};

/// One HTTP request: a GET of `path` under the workspace, or a POST of `body` to it.
async fn http(w: &World, path: &str, body: Option<Value>) -> Result<(u16, String)> {
    let url = format!("{}/api/workspaces/{}/{path}", w.root, w.ws);
    let request = match body {
        Some(body) => w.http.post(url).json(&body),
        None => w.http.get(url),
    };
    let reply = request.bearer_auth(&w.bot).send().await?;
    Ok((reply.status().as_u16(), reply.text().await?))
}

/// A read has no key links and no key blocking at any depth.
fn no_links(name: &str, read: &Value) -> Result<()> {
    ensure!(
        without_key(read, "links") && without_key(read, "blocking"),
        "{name}: {read}"
    );
    Ok(())
}

// ac:u6ijlvvc
// test:qains2w9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn links_and_links_of_are_removed_from_the_live_api() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    for address in ["doc:b", "doc:c", "doc:z"] {
        saved(&w, address, 0, json!({})).await?;
    }
    let related = json!({"related":{"edges":[{"to":"doc:b","revision":1},{"to":"doc:c"},
        {"to":"doc:z"}]}});
    saved(&w, "doc:a", 0, json!({"relations":related})).await?;
    let archive = json!({"op":"archive_object","target":"doc:z","revision":1,"reason":"Done"});
    w.command(&w.bot, archive).await?.map_err(msg)?;
    let links = json!({"op":"save_object","kind":"doc","id":"a","revision":1,"data":{},
        "links":[{"target":"doc:b"}],"reason":"Old field"});
    let text = "links was removed; use relations";
    ensure!(
        w.command(&w.bot, links.clone()).await? == Err(text.into()),
        "MCP links"
    );
    let body = json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":links});
    let (code, reply) = http(&w, "commands", Some(body)).await?;
    ensure!(
        (400..500).contains(&code) && reply.contains(text),
        "HTTP links: {code} {reply}"
    );
    let text = "links_of was removed; use relations_of";
    let links_of = w
        .mcp(&w.bot, "joppa_objects", json!({"links_of":"doc:a"}))
        .await?;
    ensure!(links_of == Err(text.into()), "MCP links_of: {links_of:?}");
    let (code, reply) = http(&w, "objects?links_of=doc:a", None).await?;
    ensure!(
        (400..500).contains(&code) && reply.contains(text),
        "HTTP links_of: {code} {reply}"
    );
    // The detail read gives the edges with their pins and the presence of the target.
    let read = w.object("doc:a").await?;
    let allowed = [
        "to",
        "title",
        "when",
        "revision",
        "current_revision",
        "present",
    ];
    let items = relations(&read)["related"]["edges"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(items.len() == 3, "{read}");
    for edge in &items {
        let keys = edge
            .as_object()
            .map(|m| m.keys().cloned().collect::<Vec<_>>());
        ensure!(
            keys.unwrap_or_default()
                .iter()
                .all(|k| allowed.contains(&k.as_str())),
            "{edge}"
        );
        let pinned = edge.get("revision").is_some();
        ensure!(edge.get("current_revision").is_some() == pinned, "{edge}");
        ensure!(
            (edge["present"] == false) == (edge["to"] == "doc:z"),
            "{edge}"
        );
    }
    ensure!(
        items[0]["revision"] == 1 && items[0]["current_revision"] == 1,
        "{read}"
    );
    // A write accepts the edges of a read back.
    change(
        &w,
        "doc:a",
        json!({"relations":relations(&read),"body":"Same edges"}),
    )
    .await?;
    let again = w.object("doc:a").await?;
    ensure!(
        edges(&again, "related") == edges(&read, "related"),
        "{again}"
    );
    let b = w.object("doc:b").await?;
    let group = json!({"direction":"incoming","relation":"related","kind":"doc","count":1});
    let groups = b["relation_groups"].as_array().cloned().unwrap_or_default();
    ensure!(
        groups.contains(&group) && b.get("link_groups").is_none(),
        "{b}"
    );
    let list = json!({"kind":"doc","fields":["relations"],"limit":25});
    let list = w.mcp(&w.bot, "joppa_objects", list).await?.map_err(msg)?;
    let a = list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| i["address"] == "doc:a");
    ensure!(a.is_some_and(|a| edges(a, "related").len() == 3), "{list}");
    let (code, document) = http(&w, "objects?address=doc:a", None).await?;
    ensure!(code == 200, "{code} {document}");
    for (name, read) in [("A", &again), ("B", &b), ("list", &list)] {
        no_links(name, read)?;
    }
    no_links("HTTP A", &serde_json::from_str(&document)?)
}

// ac:s51at8yc
// test:r6y4ah5p
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_edge_takes_a_public_item_id() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    saved(&w, "doc:b", 0, json!({})).await?;
    let sql = "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind='doc' AND local_id='b'";
    let p: String = w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0);
    saved(
        &w,
        "doc:a",
        0,
        json!({"relations":{"related":{"edges":[{"to":p}]}}}),
    )
    .await?;
    let event = last_event(&w).await?;
    ensure!(
        event["relations"]["related"]["edges"][0]["to"] == "doc:b",
        "journal: {event}"
    );
    let mcp = w.object("doc:a").await?;
    ensure!(edges(&mcp, "related") == [to("doc:b")], "MCP: {mcp}");
    let (code, text) = http(&w, "objects?address=doc:a", None).await?;
    let document: Value = serde_json::from_str(&text)?;
    ensure!(
        code == 200 && edges(&document, "related") == [to("doc:b")],
        "HTTP: {text}"
    );
    let page = json!({"relations_of":"doc:a","limit":25});
    let page = w.mcp(&w.bot, "joppa_objects", page).await?.map_err(msg)?;
    let addresses: Vec<&Value> = page["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| &i["address"])
        .collect();
    ensure!(addresses == [&json!("doc:b")], "relations_of: {page}");
    let b = w.object("doc:b").await?;
    let group = json!({"direction":"incoming","relation":"related","kind":"doc","count":1});
    let incoming = b["relation_groups"]
        .as_array()
        .is_some_and(|g| g.contains(&group));
    ensure!(incoming, "B reads the edge from A: {b}");
    Ok(())
}
