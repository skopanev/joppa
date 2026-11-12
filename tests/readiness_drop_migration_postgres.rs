//! Migration 067 drops readiness. The test builds the rows of release 066: the column
//! work_ready of joppa.objects, and work_ready and waits_for in rollup. The journal holds
//! depends_on edges. `joppa migrate` runs, then the server start replays the journal. Each
//! document stays the same, and a depends_on cycle is still refused.
#[path = "common/app_role.rs"]
mod app_role;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::{A, World};
use relations_world::{edges, refused, save, saved, stored, to};
use serde_json::{Value, json};

const B: &str = "ac:b";
const T: &str = "task:t";

/// The rows of release 066: the column work_ready, and readiness in the rollup of each row.
/// B and T depend on A.
const RELEASE_066: &str =
    "ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS work_ready boolean NOT NULL DEFAULT true";
const ROLLUP_066: &str = "UPDATE joppa.objects SET work_ready = address <> ALL($2), rollup = COALESCE(rollup,'{}'::jsonb) || CASE WHEN address = ANY($2) THEN jsonb_build_object('work_ready',false,'waits_for',jsonb_build_object('address',$3::text,'count',1)) ELSE jsonb_build_object('work_ready',true) END WHERE workspace=$1";

/// One depends_on edge to `target`.
fn depends_on(target: &str) -> Value {
    json!({"relations":{"depends_on":{"edges":[{"to":target}]}}})
}

/// The stored document of each object of W, in address order.
async fn documents(w: &World) -> Result<Vec<(String, Value)>> {
    let sql =
        "SELECT address,document::text FROM joppa.objects WHERE workspace=$1 ORDER BY address";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    let mut out = Vec::new();
    for row in rows {
        out.push((row.get(0), serde_json::from_str(row.get(1))?));
    }
    Ok(out)
}

/// The number of rows of joppa.objects that keep work_ready or waits_for in rollup, and the
/// number of columns work_ready of joppa.objects.
async fn readiness(w: &World) -> Result<(i64, i64)> {
    let conn = w.pool.get().await?;
    let sql = "SELECT count(*) FROM joppa.objects WHERE rollup::text LIKE '%\"work\\_ready\"%' OR rollup::text LIKE '%\"waits\\_for\"%'";
    let rows: i64 = conn.query_one(sql, &[]).await?.get(0);
    let sql = "SELECT count(*) FROM information_schema.columns WHERE table_schema='joppa' AND table_name='objects' AND column_name='work_ready'";
    let columns: i64 = conn.query_one(sql, &[]).await?.get(0);
    Ok((rows, columns))
}

// ac:ytw477w3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn migration_067_drops_readiness_and_the_journal_replays_to_the_same_documents() -> Result<()>
{
    let mut w = relations_world::world(&["tests/readiness_drop.rs > a"]).await?;
    saved(&w, "requirement:q", 0, json!({})).await?;
    let mut b = depends_on(A);
    b["parent"] = json!("requirement:q");
    saved(&w, B, 0, b).await?;
    let mut t = depends_on(B);
    t["parent"] = json!("requirement:q");
    saved(&w, T, 0, t).await?;
    let journal = "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command::text LIKE '%depends\\_on%'";
    let held: i64 = w
        .pool
        .get()
        .await?
        .query_one(journal, &[&w.ws])
        .await?
        .get(0);
    ensure!(held >= 2, "the journal holds the depends_on edges: {held}");
    w.stop();
    let conn = w.pool.get().await?;
    conn.batch_execute(RELEASE_066).await?;
    let waiting = vec![B.to_owned(), T.to_owned()];
    conn.execute(ROLLUP_066, &[&w.ws, &waiting, &A]).await?;
    drop(conn);
    let (rows, columns) = readiness(&w).await?;
    ensure!(
        rows >= 3 && columns == 1,
        "the rows of 066: {rows} {columns}"
    );
    let older = documents(&w).await?;

    app_role::migrate()?;
    let (rows, columns) = readiness(&w).await?;
    ensure!(columns == 0, "joppa.objects keeps the column work_ready");
    ensure!(
        rows == 0,
        "{rows} rows keep work_ready or waits_for in rollup"
    );
    let before = documents(&w).await?;
    // The server start replays the journal of W.
    let sql = "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1";
    w.pool.get().await?.execute(sql, &[&w.ws]).await?;
    w.start().await?;
    let sql = "SELECT first_view <> '{}'::jsonb FROM joppa.board_cache WHERE workspace=$1";
    let replayed: bool = w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0);
    ensure!(replayed, "the server start replays the journal");
    let after = documents(&w).await?;
    ensure!(
        after == before,
        "the replay changed a document:\n{before:?}\n{after:?}"
    );
    let relations = |all: &[(String, Value)]| -> Vec<Value> {
        all.iter().map(|(_, d)| d["relations"].clone()).collect()
    };
    ensure!(
        relations(&after) == relations(&older),
        "the relations changed"
    );
    let (rows, _) = readiness(&w).await?;
    ensure!(rows == 0, "the replay writes readiness in {rows} rows");
    for (address, target) in [(B, A), (T, B)] {
        let read = w.object(address).await?;
        ensure!(
            edges(&read, "depends_on") == [to(target)],
            "{address}: {read}"
        );
    }
    // A depends_on edge from A to B closes the cycle A, B, A.
    let revision = stored(&w, A).await?.0;
    refused(
        save(&w, A, revision, depends_on(B)).await?,
        "dependency cycle",
    )?;
    Ok(())
}
