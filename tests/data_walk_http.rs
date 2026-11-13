//! ac:ymqwcyni over a real server: POST /api/data walks the live tree in tree order. It skips
//! an archived node and a node that fails the filter, and it does not walk below them. kind
//! selects the items and does not stop the walk.
#[path = "common/data_tree.rs"]
mod data_tree;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use read_key::{addresses, data, read, with_key};
use record_world::World;
use serde_json::json;

/// The tree of data_tree.rs without the quagga nodes.
async fn tree(w: &World) -> Result<()> {
    data_tree::walk_tree(w, false).await
}

/// Check that the reply to `body` lists exactly `wanted`, in this order.
async fn lists(w: &World, key: &str, body: serde_json::Value, wanted: &[&str]) -> Result<()> {
    let reply = read(w, key, body.clone()).await?;
    let found = addresses(&reply);
    ensure!(found == wanted, "{body}: {found:?}, not {wanted:?}");
    Ok(())
}

// ac:ymqwcyni
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_walk_keeps_tree_order_and_prunes_archived_and_filtered_branches() -> Result<()> {
    with_key(tree, async |w, key| {
        let body = json!({"root":"box:root","kind":["type_a","type_b"],"filter":{"env":"prod"}});
        let wanted = ["type_a:a3", "type_b:b3", "type_a:a1", "type_b:b1"];
        lists(w, key, body, &wanted).await
    })
    .await
}

// ac:ymqwcyni
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn kind_type_b_selects_only_the_kept_type_b_nodes_in_tree_order() -> Result<()> {
    with_key(tree, async |w, key| {
        let body = json!({"root":"box:root","kind":"type_b","filter":{"env":"prod"}});
        lists(w, key, body, &["type_b:b3", "type_b:b1"]).await
    })
    .await
}

// ac:ymqwcyni
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn without_a_root_the_walk_starts_at_every_root_and_kind_does_not_stop_it() -> Result<()> {
    with_key(tree, async |w, key| {
        let body = json!({"kind":"type_a","filter":{"env":"prod"}});
        lists(w, key, body, &["type_a:a3", "type_a:a1", "type_a:c1"]).await
    })
    .await
}

// ac:ymqwcyni
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_root_that_fails_the_filter_gives_no_items() -> Result<()> {
    with_key(tree, async |w, key| {
        let body = json!({"root":"type_a:a2","filter":{"env":"prod"}});
        let reply = read(w, key, body).await?;
        ensure!(reply["items"] == json!([]), "{reply}");
        Ok(())
    })
    .await
}

// ac:ymqwcyni
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_or_unknown_root_gives_404_and_an_unknown_kind_gives_400() -> Result<()> {
    with_key(tree, async |w, key| {
        for (body, wanted) in [
            (json!({"root":"type_a:a4"}), 404),
            (json!({"root":"type_a:zzzz"}), 404),
            (json!({"kind":"nope"}), 400),
        ] {
            let (code, reply) = data(w, Some(key), &body).await?;
            ensure!(code == wanted, "{body}: {code} {reply}, not {wanted}");
            ensure!(reply.get("items").is_none(), "{body}: {reply}");
        }
        Ok(())
    })
    .await
}
