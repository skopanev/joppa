//! count: true over a real server gives only the total of the filter, in joppa_objects and in
//! GET /objects. requirement:r holds ac:a, ac:b, ac:c and 30 tasks, and alex holds 10 tasks.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{msg, saved};
use serde_json::{Value, json};

/// The 3 criteria and the 30 tasks below requirement:r. Tasks 1 to 10 are of alex.
async fn fixture(w: &World) -> Result<()> {
    for address in ["ac:b", "ac:c"] {
        saved(w, address, 0, json!({"parent":"requirement:r"})).await?;
    }
    for n in 1..=30 {
        let mut extra = json!({"parent":"requirement:r"});
        if n <= 10 {
            extra["assignee"] = json!("alex");
        }
        saved(w, &format!("task:t{n:02}"), 0, extra).await?;
    }
    Ok(())
}

/// The reply of joppa_objects with `args`, which must succeed.
async fn objects(w: &World, args: Value) -> Result<Value> {
    w.mcp(&w.bot, "joppa_objects", args).await?.map_err(msg)
}

// ac:m9lcgtsd
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn count_true_gives_only_the_total_of_the_filter() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    fixture(&w).await?;
    let all = json!({"parent":"requirement:r","depth":100,"count":true});
    let reply = objects(&w, all).await?;
    ensure!(reply == json!({"total":33}), "all below R: {reply}");
    let alex = json!({"parent":"requirement:r","depth":100,"count":true,
        "kind":"task","assignee":"alex"});
    let reply = objects(&w, alex).await?;
    ensure!(reply == json!({"total":10}), "the tasks of alex: {reply}");
    let path = "/objects?parent=requirement:r&depth=100&count=true";
    let reply = blue_world::get(&w, &w.bot, path).await?;
    ensure!(reply == json!({"total":33}), "GET {path}: {reply}");
    let first = json!({"parent":"requirement:r","depth":100,"limit":1});
    let cursor = objects(&w, first).await?["next_cursor"].clone();
    ensure!(
        cursor.is_string(),
        "a page of 1 gives next_cursor: {cursor}"
    );
    // Each parameter that pages, picks or reads edges cannot go with count.
    let under = |key: &str, value: Value| {
        let mut args = json!({"parent":"requirement:r","depth":100,"count":true});
        args[key] = value;
        (key.to_owned(), args)
    };
    let alone = |key: &str, value: Value| (key.to_owned(), json!({"count":true, key: value}));
    for (key, args) in [
        alone("address", json!("ac:a")),
        under("cursor", cursor),
        under("limit", json!(5)),
        under("fields", json!(["title"])),
        alone("relations_of", json!("ac:a")),
        alone("in_relation", json!("depends_on")),
        alone("related_to", json!("ac:a")),
    ] {
        let reply = w.mcp(&w.bot, "joppa_objects", args.clone()).await?;
        let named = reply.as_ref().is_err_and(|e| e.contains(&key));
        ensure!(
            named,
            "count with {key} is refused and names {key}: {args} {reply:?}"
        );
    }
    Ok(())
}
