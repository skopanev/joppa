//! requirement:node-filters over a real server: joppa_objects, joppa_search and their HTTP
//! routes take the same node filters and keep the same nodes.
#[path = "common/node_filter_world.rs"]
mod node_filter_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use node_filter_world::{kept, seed, sorted};
use serde_json::{Value, json};

// node-filters-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn parent_gives_the_whole_subtree_and_depth_limits_it() -> Result<()> {
    let w = record_world::world(&[]).await?;
    seed(&w).await?;
    for (filters, expected) in [
        (
            json!({"parent":"requirement:r"}),
            vec!["ac:a", "ac:b", "rule:x", "rule:z"],
        ),
        (
            json!({"parent":"requirement:r","depth":1}),
            vec!["ac:a", "ac:b"],
        ),
        (
            json!({"parent":"requirement:r","depth":2}),
            vec!["ac:a", "ac:b", "rule:x"],
        ),
        (json!({"parent":"ac:a"}), vec!["rule:x", "rule:z"]),
        (json!({"parent":"rule:z"}), vec![]),
        // Without a parent address, depth counts from the top: 2 levels hold r, a and b.
        (
            json!({"depth":2,"kind":["ac","rule"]}),
            vec!["ac:a", "ac:b"],
        ),
        (
            json!({"parent":"*","depth":3,"kind":"rule"}),
            vec!["rule:x"],
        ),
    ] {
        ensure!(kept(&w, filters.clone()).await? == expected, "{filters}");
    }
    // Without a filter the list gives the roots; parent "*" gives every level.
    let roots = w
        .mcp(&w.bot, "joppa_objects", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let all = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"*","kind":["ac","rule"]}),
        )
        .await?;
    let all = all.map_err(anyhow::Error::msg)?;
    let address = |i: &Value| i["address"].as_str().map(str::to_owned);
    ensure!(
        sorted(&roots["items"], address)
            .iter()
            .all(|a| !a.starts_with("ac:") && !a.starts_with("rule:"))
            && sorted(&all["items"], address) == ["ac:a", "ac:b", "rule:x", "rule:z"],
        "{roots} {all}"
    );
    for (tool, mut args, error) in [
        (
            "joppa_objects",
            json!({"parent":"requirement:r","depth":0}),
            "depth must be an integer from 1 to 100",
        ),
        (
            "joppa_objects",
            json!({"depth":101}),
            "depth must be an integer from 1 to 100",
        ),
        (
            "joppa_search",
            json!({"depth":"x"}),
            "depth must be an integer from 1 to 100",
        ),
    ] {
        if tool == "joppa_search" {
            args["query"] = json!("fixture");
        }
        let reply = w.mcp(&w.bot, tool, args.clone()).await?;
        ensure!(reply == Err(error.into()), "{tool} {args}: {reply:?}");
    }
    let bad = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?depth=x",
            w.root, w.ws
        ))
        .bearer_auth(&w.bot)
        .send()
        .await?;
    let status = bad.status();
    let body: Value = bad.json().await?;
    ensure!(
        status == 409 && body["error"] == "depth must be an integer from 1 to 100",
        "{status} {body}"
    );
    Ok(())
}
