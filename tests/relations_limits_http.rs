//! requirement:q438uw0s over a real server: the relations of one node have size limits.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use relations_world::{msg, refused, relations, save, saved, schema, stored};
use serde_json::{Map, Value, json};

/// A relations value with the keys `keys`, each without edges.
fn keys(keys: &[String]) -> Map<String, Value> {
    keys.iter()
        .map(|k| (k.clone(), json!({"edges":[]})))
        .collect()
}

// ac:jey19ie7
// test:pzay5s8q
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn relations_have_size_limits() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    // W declares the flow relations r1..r63: 65 keys with depends_on and related.
    let flows: Vec<String> = (1..=63).map(|i| format!("r{i}")).collect();
    let put: Map<String, Value> = flows
        .iter()
        .map(|k| (k.clone(), json!({"title":k.to_uppercase()})))
        .collect();
    schema(&w, json!({"put_relations":put}))
        .await?
        .map_err(msg)?;
    let mut all = vec!["depends_on".to_owned(), "related".to_owned()];
    all.extend(flows.iter().cloned());
    // 201 target nodes, written through the store.
    let targets: Vec<String> = (0..201).map(|i| format!("doc:t{i:03}")).collect();
    for target in &targets {
        let id = target.trim_start_matches("doc:");
        let command = json!({"op":"save_object","kind":"doc","id":id,"revision":0,"parent":null,
            "data":{"title":id,"owner":"owner"},"reason":"Edge target"});
        let request = uuid::Uuid::new_v4().to_string();
        joppa::store::execute_remote_input(&w.pool, &w.ws, "bot", &request, &command).await?;
    }
    saved(&w, "doc:a", 0, json!({})).await?;
    let to = |n: usize| -> Vec<Value> { targets[..n].iter().map(|t| json!({"to":t})).collect() };
    let long = |n: usize| "x".repeat(n);
    for (relations, error) in [
        (json!(keys(&all)), "a node holds at most 64 relations"),
        (
            json!({"r1":{"edges":to(201)}}),
            "a relation holds at most 200 edges",
        ),
        (
            json!({"r1":{"title":long(201),"edges":[]}}),
            "title holds at most 200 characters",
        ),
        (
            json!({"r1":{"edges":[{"to":targets[0],"when":long(201)}]}}),
            "when holds at most 200 characters",
        ),
    ] {
        let reply = save(&w, "doc:a", 1, json!({"relations":relations})).await?;
        refused(reply, error)?;
    }
    // 64 keys, 200 edges in one key and texts of 200 characters are stored.
    let mut most = keys(&all[..64]);
    let mut edges = to(200);
    edges[0]["when"] = json!(long(200));
    most.insert("r1".into(), json!({"title":long(200),"edges":edges}));
    saved(&w, "doc:a", 1, json!({"relations":most})).await?;
    let document = json!({"document":stored(&w, "doc:a").await?.2});
    let stored = relations(&document);
    let r1 = &stored["r1"];
    ensure!(
        stored.as_object().map(Map::len) == Some(64)
            && r1["edges"].as_array().map(Vec::len) == Some(200)
            && r1["title"].as_str().map(str::len) == Some(200)
            && r1["edges"][0]["when"].as_str().map(str::len) == Some(200),
        "the last save is stored: {} keys",
        stored.as_object().map_or(0, Map::len)
    );
    Ok(())
}
