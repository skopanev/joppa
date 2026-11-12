//! requirement:q438uw0s over a real server: depends_on refuses cycles, a flow allows them, and
//! related_to lists only the depends_on and related neighbours.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::{A, World};
use relations_world::{change, declare, msg, refused, saved};
use serde_json::{Value, json};

/// The addresses that joppa_objects lists for `filters`.
async fn listed(w: &World, filters: Value) -> Result<Vec<String>> {
    let page = w
        .mcp(&w.bot, "joppa_objects", filters)
        .await?
        .map_err(msg)?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect())
}

/// One edge of `relation` to `target`.
fn edge(relation: &str, target: &str) -> Value {
    json!({"relations":{relation:{"edges":[{"to":target}]}}})
}

// ac:syebi8av
// test:0n3j3bb7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn depends_on_refuses_cycles() -> Result<()> {
    // C is criterion A: it lists a test without a result, so it is blue.
    let w = relations_world::world(&["tests/c.rs > c"]).await?;
    let blue = relations_world::tone(&w, A).await?;
    ensure!(blue == "blue", "C is blue: {blue}");
    let parent = json!({"parent":"requirement:r"});
    saved(&w, "task:t1", 0, parent.clone()).await?;
    saved(&w, "task:t2", 0, parent).await?;
    change(&w, "task:t1", edge("depends_on", A)).await?;
    let revision = relations_world::stored(&w, A).await?.0;
    let cycle = relations_world::save(&w, A, revision, edge("depends_on", "task:t1")).await?;
    refused(cycle, "dependency cycle")?;
    change(&w, "task:t2", edge("related", A)).await?;
    Ok(())
}

// ac:4tkeo6nc
// test:5c4823x0
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_flow_allows_cycles_and_blocks_nothing() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    let parent = json!({"parent":"requirement:r"});
    saved(&w, "task:t1", 0, parent.clone()).await?;
    saved(&w, "task:t2", 0, parent).await?;
    change(&w, "task:t1", edge("swap", "task:t2")).await?;
    change(&w, "task:t2", edge("swap", "task:t1")).await?;
    for address in ["task:t1", "task:t2"] {
        let read = w.object(address).await?;
        let other = if address == "task:t1" {
            "task:t2"
        } else {
            "task:t1"
        };
        let stored = relations_world::edges(&read, "swap");
        ensure!(stored == [relations_world::to(other)], "{read}");
    }
    Ok(())
}

// ac:vp0gqqm5
// test:t20krobe
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn related_to_lists_only_depends_on_and_related_neighbours() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    for address in ["doc:b", "doc:c", "doc:d"] {
        saved(&w, address, 0, json!({})).await?;
    }
    let relations = json!({"related":{"edges":[{"to":"doc:b"}]},
        "depends_on":{"edges":[{"to":"doc:c"}]},"swap":{"edges":[{"to":"doc:d"}]}});
    saved(&w, "doc:a", 0, json!({"relations":relations})).await?;
    let mut found = listed(&w, json!({"related_to":"doc:a","limit":25})).await?;
    found.sort();
    ensure!(found == ["doc:b", "doc:c"], "{found:?}");
    Ok(())
}
