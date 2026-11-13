//! ac:0rglqvf9 over a real server: with query, POST /api/data ranks only the walked nodes with
//! the hybrid search. limit and query have bounds.
#[path = "common/data_tree.rs"]
mod data_tree;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use read_key::{addresses, data, read, with_key};
use record_world::World;
use serde_json::{Value, json};

/// The tree of data_tree.rs with the 12 quagga nodes.
async fn tree(w: &World) -> Result<()> {
    data_tree::walk_tree(w, true).await
}

/// The addresses that joppa_search gives for `query` and `limit`, once it finds at least
/// `least` items. The search index fills after the writes.
async fn searched(w: &World, query: &str, limit: u64, least: usize) -> Result<Vec<String>> {
    let mut found = vec![];
    for _ in 0..200 {
        let args = json!({"query":query,"limit":limit});
        let page = w.mcp(&w.bot, "joppa_search", args).await?;
        found = addresses(&page.map_err(anyhow::Error::msg)?);
        if found.len() >= least {
            return Ok(found);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    anyhow::bail!("joppa_search {query:?} found only {found:?}")
}

// ac:0rglqvf9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_query_finds_only_the_walked_node_and_no_archived_or_other_branch() -> Result<()> {
    with_key(tree, async |w, key| {
        // joppa_search finds a1, b2 and c1 outside the filter and the root.
        searched(w, "zebrafish", 25, 3).await?;
        let body = json!({"root":"box:root","filter":{"env":"prod"},"query":"zebrafish"});
        let found = addresses(&read(w, key, body).await?);
        // The search text of a node holds its ancestors, so b1 matches through a1.
        ensure!(found == ["type_a:a1", "type_b:b1"], "{found:?}");
        Ok(())
    })
    .await
}

// ac:0rglqvf9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_query_gives_10_items_by_default_and_25_at_most_in_the_order_of_joppa_search()
-> Result<()> {
    with_key(tree, async |w, key| {
        let walked = addresses(&read(w, key, json!({"root":"box:root"})).await?);
        for (limit, count) in [(None, 10), (Some(25), 12)] {
            let search = searched(w, "quagga", limit.unwrap_or(10), count).await?;
            let wanted: Vec<String> = search
                .into_iter()
                .filter(|a| walked.contains(a))
                .take(count)
                .collect();
            let mut body = json!({"root":"box:root","query":"quagga"});
            if let Some(limit) = limit {
                body["limit"] = json!(limit);
            }
            let found = addresses(&read(w, key, body.clone()).await?);
            ensure!(found.len() == count, "{body}: {} items", found.len());
            ensure!(
                found == wanted,
                "{body}: {found:?}, joppa_search {wanted:?}"
            );
        }
        Ok(())
    })
    .await
}

// ac:0rglqvf9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_blank_or_long_query_and_a_limit_out_of_range_give_400() -> Result<()> {
    with_key(tree, async |w, key| {
        let long = "q".repeat(2001);
        let bodies: [Value; 6] = [
            json!({"query":"   "}),
            json!({"query":long}),
            json!({"query":"quagga","limit":0}),
            json!({"query":"quagga","limit":26}),
            json!({"limit":0}),
            json!({"limit":1001}),
        ];
        for mut body in bodies {
            body["root"] = json!("box:root");
            let (code, reply) = data(w, Some(key), &body).await?;
            let shown = body.to_string().chars().take(80).collect::<String>();
            ensure!(code == 400, "{shown}: {code} {reply}");
        }
        Ok(())
    })
    .await
}

// ac:0rglqvf9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_limit_without_a_query_keeps_the_first_items_of_the_walk() -> Result<()> {
    with_key(tree, async |w, key| {
        let body = json!({"root":"box:root","kind":["type_a","type_b"],"limit":2});
        let found = addresses(&read(w, key, body).await?);
        ensure!(found == ["type_a:a3", "type_b:b3"], "{found:?}");
        Ok(())
    })
    .await
}
