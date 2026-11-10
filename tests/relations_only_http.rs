//! A write over a real server stores relations and no rollback shadow `links`: the current
//! document, each revision of the state row and the rows of joppa.object_links.
#[path = "common/older_links.rs"]
mod older_links;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use older_links::flat;
use record_world::World;
use relations_world::{change, saved, stored};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// One edge as (key, to, pin).
type Edge = (String, String, Option<u64>);

/// The document of `address` holds the relations `edges` and no links, and no revision of its
/// state row holds links, in value or in compact. A node without edges holds relations {}.
async fn relations_only(w: &World, address: &str, edges: &[Edge]) -> Result<()> {
    let document = stored(w, address).await?.2;
    let relations = &document["relations"];
    let right = relations.is_object()
        && flat(relations) == edges
        && (!edges.is_empty() || *relations == json!({}))
        && document.get("links").is_none();
    ensure!(right, "document {address}: {document}");
    let sql = "SELECT value::text,COALESCE(compact,value)::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key=$2";
    let row = w
        .pool
        .get()
        .await?
        .query_one(sql, &[&w.ws, &address])
        .await?;
    for column in [0, 1] {
        let value: Value = serde_json::from_str(row.get(column))?;
        for revision in value["revisions"].as_array().into_iter().flatten() {
            ensure!(
                revision.get("links").is_none(),
                "state row {address}: {revision}"
            );
        }
    }
    Ok(())
}

// ac:m88fiwaq
// test:xw5nzyyj
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_write_stores_relations_and_no_links() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    for address in ["doc:b", "doc:c", "doc:a", "doc:e"] {
        saved(&w, address, 0, json!({})).await?;
    }
    relations_only(&w, "doc:e", &[]).await?;
    let relations = json!({"depends_on":{"edges":[{"to":"doc:b","revision":1}]},
        "related":{"edges":[{"to":"doc:c"}]}});
    let edges = [
        ("depends_on".to_owned(), "doc:b".to_owned(), Some(1)),
        ("related".to_owned(), "doc:c".to_owned(), None),
    ];
    change(&w, "doc:a", json!({"relations":relations})).await?;
    relations_only(&w, "doc:a", &edges).await?;
    change(&w, "doc:a", json!({"body":"Only new data"})).await?;
    relations_only(&w, "doc:a", &edges).await?;
    change(&w, "doc:e", json!({"body":"Only new data"})).await?;
    relations_only(&w, "doc:e", &[]).await?;

    let sql = "SELECT target,relation,revision FROM joppa.object_links WHERE workspace=$1 AND source='doc:a'";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    let found: BTreeSet<(String, String, Option<i32>)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    let expected = BTreeSet::from([
        ("doc:b".to_owned(), "depends_on".to_owned(), Some(1)),
        ("doc:c".to_owned(), "related".to_owned(), None),
    ]);
    ensure!(
        rows.len() == 2 && found == expected,
        "object_links: {found:?}"
    );
    let sql = "SELECT count(*) FROM information_schema.columns WHERE table_schema='joppa' AND table_name='object_links' AND column_name='blocking'";
    let blocking: i64 = w.pool.get().await?.query_one(sql, &[]).await?.get(0);
    ensure!(blocking == 0, "object_links keeps the column blocking");
    Ok(())
}
