//! Migration 064 removes the rollback shadow `links` of migration 062 without a replay. The test
//! builds the rows of release 05e8b4a: the older form of the 062 test (older_links.rs) with the
//! blocking column of object_links, then the data pass of 062. N2 has a flow, and SQL gives its
//! document and its state row the shadow links of its relations. `joppa migrate` runs as the
//! install role, then the server starts twice as the application role of server/install.sh.
//! A rebuild from the journal must write the same rows as 064.
#[path = "common/app_role.rs"]
mod app_role;
#[path = "common/state_compare.rs"]
mod compare;
#[path = "common/older_links.rs"]
mod older_links;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use app_role::{AppRole, migrate};
use older_links::{ROWS, doc, flat, snapshot, store_older_form_but, wanted};
use record_world::World;
use relations_world::{change, declare, relations, saved, stored, without_key};
use serde_json::{Value, json};

/// N0 has no edges, N1 has the links of the fixture, and N2 has a flow.
const N2: &str = "doc:flow";
/// The rows that 064 must keep.
const KEPT: [&str; 4] = [
    "events",
    "object_revisions",
    "state_revisions",
    "command_receipts",
];
/// The 05e8b4a schema of object_links still has the blocking column.
const OLDER_SCHEMA: &str = "ALTER TABLE joppa.object_links ADD COLUMN IF NOT EXISTS blocking boolean NOT NULL DEFAULT false; ALTER TABLE joppa.object_links ALTER COLUMN blocking SET DEFAULT false";

/// Each revision of a state row value `v` with the shadow links of its relations.
fn shadowed(v: &str) -> String {
    format!(
        "jsonb_set({v}, '{{revisions}}', (SELECT jsonb_agg(r || jsonb_build_object('links', joppa.links_shadow(r->'relations')) ORDER BY i) FROM jsonb_array_elements({v}->'revisions') WITH ORDINALITY t(r, i)))"
    )
}

/// Give the document and the state row of N2 the shadow links, as release 05e8b4a stored them.
async fn shadow_n2(w: &World) -> Result<()> {
    let conn = w.pool.get().await?;
    let sql = "UPDATE joppa.objects SET document=document||jsonb_build_object('links',joppa.links_shadow(document->'relations')) WHERE workspace=$1 AND address=$2";
    conn.execute(sql, &[&w.ws, &N2]).await?;
    let (value, compact) = (shadowed("value"), shadowed("compact"));
    let sql = format!(
        "UPDATE joppa.state_entries SET value={value},compact=CASE WHEN compact IS NULL THEN NULL ELSE {compact} END WHERE workspace=$1 AND section='objects' AND key=$2"
    );
    conn.execute(&sql, &[&w.ws, &N2]).await?;
    let sql = "SELECT jsonb_array_length(document->'links') FROM joppa.objects WHERE workspace=$1 AND address=$2";
    let shadow: i32 = conn.query_one(sql, &[&w.ws, &N2]).await?.get(0);
    ensure!(shadow > 0, "N2 stores the shadow links");
    Ok(())
}

/// The normalized rows of W in joppa.objects and joppa.state_entries.
async fn rows(w: &World) -> Result<Vec<(String, Vec<Value>)>> {
    let names = compare::names(&w.pool, &w.ws).await?;
    let conn = w.pool.get().await?;
    let mut out = Vec::new();
    for (table, order) in [("objects", "address"), ("state_entries", "section,key")] {
        let sql = format!(
            "SELECT row_to_json(t)::text FROM joppa.{table} t WHERE workspace=$1 ORDER BY {order}"
        );
        let mut found = Vec::new();
        for row in conn.query(&sql, &[&w.ws]).await? {
            let value: Value = serde_json::from_str(row.get(0))?;
            found.push(compare::normalize(&value, &names)?);
        }
        out.push((table.to_owned(), found));
    }
    Ok(out)
}

/// The rows of object_links of W, without the blocking column.
async fn links(w: &World) -> Result<Vec<String>> {
    let sql = "SELECT concat_ws(' ',source,target,relation,revision,when_label) FROM joppa.object_links WHERE workspace=$1 ORDER BY 1";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// The reads and the rows of W after 064. `n2` is the read of N2 before the migration.
async fn relations_only(w: &World, n2: &Value) -> Result<()> {
    for n in 1..=30 {
        let read = w.object(&doc(n)).await?;
        let right = flat(&relations(&read)) == wanted(&doc(n)) && without_key(&read, "links");
        ensure!(right, "{}: {read}", doc(n));
    }
    let read = w.object(N2).await?;
    let right = relations(&read) == *n2 && without_key(&read, "links");
    ensure!(right, "{N2}: {read}, before {n2}");
    let sql = "SELECT address,document::text FROM joppa.objects WHERE workspace=$1 AND kind='doc'";
    for row in w.pool.get().await?.query(sql, &[&w.ws]).await? {
        let address: String = row.get(0);
        let document: Value = serde_json::from_str(row.get(1))?;
        // N0 holds relations {}, each other node the relations of its read.
        let wanted = if address == N2 {
            flat(n2)
        } else {
            wanted(&address)
        };
        let right = document["relations"].is_object()
            && flat(&document["relations"]) == wanted
            && (address != doc(30) || document["relations"] == json!({}))
            && document.get("links").is_none();
        ensure!(right, "document {address}: {document}");
    }
    let sql = "SELECT key FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND (jsonb_path_exists(value,'$.revisions[*].links') OR jsonb_path_exists(COALESCE(compact,'{}'::jsonb),'$.revisions[*].links'))";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    let keys: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    ensure!(keys.is_empty(), "state rows with links: {keys:?}");
    let sql = "SELECT first_view->>'projection_version' FROM joppa.board_cache WHERE workspace=$1";
    let version: Option<String> = w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0);
    ensure!(version.as_deref() == Some("22"), "version {version:?}");
    let sql = "SELECT count(*) FROM information_schema.columns WHERE table_schema='joppa' AND table_name='object_links' AND column_name='blocking'";
    let blocking: i64 = w.pool.get().await?.query_one(sql, &[]).await?.get(0);
    ensure!(blocking == 0, "object_links keeps the column blocking");
    Ok(())
}

// ac:lte9e1y5
// test:q8qmuq2q
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn migration_064_removes_the_shadow_links_without_a_replay() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    declare(&w, &[("feeds", "Feeds")]).await?;
    for n in 1..=30 {
        saved(&w, &doc(n), 0, json!({})).await?;
    }
    let flow = json!({"feeds":{"edges":[{"to":doc(2)}]},"related":{"edges":[{"to":doc(3)}]}});
    saved(&w, N2, 0, json!({"relations":flow})).await?;
    let n2 = relations(&w.object(N2).await?);
    ensure!(n2["feeds"]["edges"][0]["to"] == doc(2).as_str(), "{n2}");
    w.stop();
    // The rows of release 05e8b4a.
    w.pool.get().await?.batch_execute(OLDER_SCHEMA).await?;
    store_older_form_but(&w, &[N2.to_owned()]).await?;
    let data_pass = include_str!("../migrations/062_relations_data.sql");
    w.pool.get().await?.batch_execute(data_pass).await?;
    shadow_n2(&w).await?;
    let before = snapshot(&w, &KEPT).await?;
    let links_before = links(&w).await?;

    let app = AppRole::create(&w).await?;
    let result = async {
        migrate()?;
        let server = app.start(&w).await?;
        relations_only(&w, &n2).await?;
        ensure!(
            links(&w).await? == links_before,
            "object_links keeps every row"
        );
        let kept = "events, object_revisions, state_revisions and command_receipts keep their xmin";
        ensure!(snapshot(&w, &KEPT).await? == before, "{kept}");
        // A second start writes no row.
        let first = snapshot(&w, &ROWS).await?;
        drop(server);
        let server = app.start(&w).await?;
        let second = "the second start writes a row";
        ensure!(snapshot(&w, &ROWS).await? == first, "{second}");
        // A rebuild from the journal writes the same rows as 064.
        let migrated = rows(&w).await?;
        drop(server);
        let sql = "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1";
        w.pool.get().await?.execute(sql, &[&w.ws]).await?;
        let _server = app.start(&w).await?;
        let rebuilt = rows(&w).await?;
        for ((table, a), (_, b)) in migrated.iter().zip(&rebuilt) {
            let diff = compare::difference(a, b);
            ensure!(
                diff.is_none(),
                "{table}: 064 and the rebuild differ\n{diff:?}"
            );
        }
        // A save of N0, N1 and N2 after 064 is stored.
        for address in [doc(30), doc(1), N2.to_owned()] {
            change(&w, &address, json!({"body":"Saved after 064"})).await?;
            let (revision, _, document) = stored(&w, &address).await?;
            let right = revision == 2 && document.get("links").is_none();
            ensure!(right, "{address}: {revision} {document}");
        }
        Ok(())
    }
    .await;
    app.remove(&w).await?;
    result
}
