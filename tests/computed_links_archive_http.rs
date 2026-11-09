//! requirement:8yyywrrq: a write that archives a node reaches the nodes that linked to it, so
//! their `links` values and their link tones change in the same write and equal a full
//! rebuild.
#[path = "common/computed_world.rs"]
mod computed_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use computed_world::{archive, expect, node, save, schema};
use record_world::World;
use serde_json::{Value, json};

/// The stored computed values and rollups of each object of the workspace.
async fn rows(w: &World) -> Result<Vec<(String, Option<String>, Option<String>)>> {
    let sql = "SELECT address,computed::text,rollup::text FROM joppa.objects WHERE workspace=$1 ORDER BY address";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}

/// The field stage with the source `from`.
fn stage(from: &str) -> Value {
    json!({"fields":{"stage":{"type":"enum","values":["in_progress","review","todo","done"],"from":from}}})
}

// ac:o4nk0hwx
// test:epgbzzcx
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_link_target_leaves_the_links_value_at_once() -> Result<()> {
    let w = record_world::world(&[]).await?;
    // Area x has the status rule links: its tone reads the tasks that link to it.
    let types = json!({"task":node("Tasks", "document", &["ac"], stage("own")),
        "bug":node("Bugs", "document", &[], stage("links")),
        "area":node("Areas", "document", &[], json!({"status":"links"}))});
    schema(&w, types).await?.map_err(anyhow::Error::msg)?;
    let to_x = json!({"related":{"edges":[{"to":"area:x"}]}});
    let to_tasks = json!({"related":{"edges":[{"to":"task:t1"},{"to":"task:t2"}]}});
    for (address, extra) in [
        ("area:x", json!({})),
        (
            "task:t1",
            json!({"parent":"ac:a","stage":"done","relations":to_x}),
        ),
        (
            "task:t2",
            json!({"parent":"ac:a","stage":"todo","relations":to_x}),
        ),
        ("bug:b", json!({"relations":to_tasks})),
    ] {
        save(&w, address, 0, extra)
            .await?
            .map_err(|e| anyhow::anyhow!("{address}: {e}"))?;
    }
    expect(
        &w,
        "stage",
        &[("bug:b", json!("todo"))],
        (json!("todo"), &["task:t2", "bug:b"]),
    )
    .await?;
    archive(&w, "task:t2", 1).await?;
    expect(
        &w,
        "stage",
        &[("bug:b", json!("done"))],
        (json!("done"), &["task:t1", "bug:b"]),
    )
    .await?;
    let written = rows(&w).await?;
    joppa::store::catch_up_statuses(&w.pool, true).await?;
    let rebuilt = rows(&w).await?;
    for (a, b) in written.iter().zip(&rebuilt) {
        ensure!(a == b, "partial {a:?}\nfull {b:?}");
    }
    ensure!(written.len() == rebuilt.len(), "row count");
    Ok(())
}
