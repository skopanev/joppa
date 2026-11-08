//! A vector search of the current objects reads the HNSW index of `joppa.search_documents`
//! and no row of `joppa.search_revisions`. The test builds the nearest-vector statement from
//! the text of src/search_candidates.rs at build time, with the clause of the node filters
//! from `node_filter::bind`, so it plans the statement of the code.
#[path = "common/pg_plan.rs"]
mod pg_plan;
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::embeddings::{Embedder, PROFILE};
use joppa::{node_filter, store};
use serde_json::{Value, json};

const SOURCE: &str = include_str!("../src/search_candidates.rs");

/// The nearest-vector statement without all_versions, as `candidates::nearest` builds it.
fn nearest_sql(bound: &node_filter::Bound) -> Result<String> {
    let keep = pg_plan::literal(SOURCE, "::boolean OR EXISTS(SELECT 1 FROM joppa.objects o")?;
    let query = pg_plan::literal(SOURCE, "SELECT workspace,kind,local_id,rev FROM (")?;
    ensure!(
        !keep.contains("{{") && !query.contains("{{"),
        "{keep} {query}"
    );
    let keep = keep
        .replace("{flag}", "4")
        .replace("{alias}", "d")
        .replace("{}", &bound.clause(5));
    Ok(query.replace("{revisions}", "").replace("{d}", &keep))
}

/// The rows of `table` that sequential and index scans returned, by the statistics views.
async fn tuples_read(pool: &pg_plan::Pool, table: &str) -> Result<i64> {
    let conn = pool.get().await?;
    conn.batch_execute("SELECT pg_stat_clear_snapshot()")
        .await?;
    let sql = "SELECT COALESCE(seq_tup_read,0)+COALESCE(idx_tup_fetch,0) FROM pg_stat_user_tables WHERE schemaname='joppa' AND relname=$1";
    Ok(conn.query_one(sql, &[&table]).await?.get(0))
}

/// Wait until the earlier writes of the test have reported their counts: the counts of both
/// tables stay the same for 2 s.
async fn settle(pool: &pg_plan::Pool) -> Result<()> {
    let counts = async || -> Result<(i64, i64)> {
        let documents = tuples_read(pool, "search_documents").await?;
        Ok((documents, tuples_read(pool, "search_revisions").await?))
    };
    let mut last = counts().await?;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let now = counts().await?;
        if now == last {
            return Ok(());
        }
        last = now;
    }
    anyhow::bail!("the statistics do not settle")
}

/// The revision rows read, after the search has reported its counts. A backend reports its
/// counts at most 1 s after it goes idle, so the test waits until the counts of the search
/// documents change, and then 2 s more for the other connection of the search.
async fn after(pool: &pg_plan::Pool, documents_before: i64) -> Result<i64> {
    for _ in 0..100 {
        if tuples_read(pool, "search_documents").await? != documents_before {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    tuples_read(pool, "search_revisions").await
}

// ac:walslydk
// test:1rsasscq
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn only_search_documents_has_the_hnsw_vector_index() -> Result<()> {
    let pool = pg_plan::db().await?;
    let conn = pool.get().await?;
    let sql = "SELECT c.relname,i.relname,a.amname,pg_get_indexdef(i.oid),pg_get_expr(x.indpred,x.indrelid) FROM pg_index x JOIN pg_class i ON i.oid=x.indexrelid JOIN pg_class c ON c.oid=x.indrelid JOIN pg_am a ON a.oid=i.relam JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='joppa' AND c.relname IN ('search_documents','search_revisions') ORDER BY 1,2";
    let rows = conn.query(sql, &[]).await?;
    let list: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!([
                r.get::<_, String>(0),
                r.get::<_, String>(1),
                r.get::<_, String>(2),
                r.get::<_, String>(3),
                r.get::<_, Option<String>>(4)
            ])
        })
        .collect();
    println!("{}", json!(list));
    let vector = |r: &Value| ["hnsw", "ivfflat"].contains(&r[2].as_str().unwrap_or_default());
    let documents: Vec<&Value> = list
        .iter()
        .filter(|r| r[0] == "search_documents" && vector(r))
        .collect();
    ensure!(documents.len() == 1, "{list:?}");
    let hnsw = documents[0];
    ensure!(
        hnsw[1] == "search_documents_embedding_hnsw" && hnsw[2] == "hnsw",
        "{hnsw}"
    );
    let def = hnsw[3].as_str().unwrap_or_default();
    ensure!(def.contains("(embedding vector_cosine_ops)"), "{def}");
    ensure!(hnsw[4] == "(embedding IS NOT NULL)", "{hnsw}");
    ensure!(
        !list.iter().any(|r| r[0] == "search_revisions" && vector(r)),
        "search_revisions has a vector index: {list:?}"
    );
    Ok(())
}

// ac:walslydk
// test:divkiwcb
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_nearest_vector_query_uses_the_hnsw_index_and_gives_100_rows() -> Result<()> {
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "hnsw-plan").await?;
    pg_plan::many_nodes(&pool, &ws, 2000, 0).await?;
    pg_plan::vectors(&pool, &ws).await?;
    pg_plan::analyze(&pool).await?;
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let vector: String = tx
        .query_one(
            "SELECT embedding::text FROM joppa.search_documents WHERE workspace=$1 AND local_id='d7'",
            &[&ws],
        )
        .await?
        .get(0);
    let filter = node_filter::NodeFilter::default();
    let bound = node_filter::bind(&*tx, &ws, &filter, false, false).await?;
    let sql = nearest_sql(&bound)?;
    let settings = pg_plan::literal(SOURCE, "SET LOCAL hnsw.ef_search")?;
    tx.batch_execute(&settings).await?;
    let scope = vec![ws.clone()];
    let narrows = bound.narrows();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
        vec![&scope, &PROFILE, &vector, &narrows];
    params.extend(bound.params());
    // On 2,000 rows the planner takes a Seq Scan and a Sort: the vectors are in the TOAST
    // table, so the cost model sees a small table. The test with 30,000 rows below checks the
    // choice of the planner. Here the plan without the other paths proves that the index CAN
    // serve the query: the switches only add a cost to a Seq Scan, a Bitmap Scan and a Sort,
    // so an index that does not match the order, the operator class or the partial predicate
    // leaves one of them in the plan.
    let default_plan = pg_plan::explain(&*tx, "FORMAT JSON", &sql, &params).await?;
    tx.batch_execute("SET LOCAL enable_seqscan = off; SET LOCAL enable_bitmapscan = off; SET LOCAL enable_sort = off").await?;
    let plan = pg_plan::explain(&*tx, "FORMAT JSON", &sql, &params).await?;
    let text = plan.to_string();
    let evidence = json!({"statement":sql,"plan":plan,"default_plan":default_plan});
    println!("{evidence}");
    ensure!(
        text.contains("search_documents_embedding_hnsw"),
        "{evidence}"
    );
    ensure!(!text.contains("search_revisions"), "{evidence}");
    ensure!(
        !default_plan.to_string().contains("search_revisions"),
        "{evidence}"
    );
    // The index scan gives the 100 nearest rows: the iterative scan continues past the
    // ef_search candidates until 100 rows pass the filters.
    let rows = tx.query(&sql, &params).await?;
    ensure!(rows.len() == 100, "{} rows", rows.len());
    let first: String = rows[0].get(2);
    ensure!(first == "d7", "the nearest row is {first}");
    Ok(())
}

// ac:walslydk
// test:6vwrvnsg
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_search_without_all_versions_reads_no_revision_row() -> Result<()> {
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "hnsw-search").await?;
    pg_plan::many_nodes(&pool, &ws, 300, 0).await?;
    pg_plan::vectors(&pool, &ws).await?;
    pg_plan::analyze(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    let filter = node_filter::NodeFilter::default();
    let search = async |flags: store::search::Flags| {
        settle(&pool).await?;
        let documents = tuples_read(&pool, "search_documents").await?;
        let revisions = tuples_read(&pool, "search_revisions").await?;
        let reply = store::search::find_with(
            &pool,
            "bot",
            &ws,
            "zzqqxx",
            &filter,
            50,
            Some(&embedder),
            flags,
        )
        .await?;
        let read = after(&pool, documents).await? - revisions;
        Ok::<_, anyhow::Error>((reply, read))
    };
    let flags = store::search::Flags::default();
    let (reply, read) = search(flags).await?;
    println!(
        "{}",
        json!({"revision_rows_read":read,"mode":reply["mode"],"total":reply["total"]})
    );
    ensure!(reply["mode"] == "hybrid", "{reply}");
    ensure!(read == 0, "the search read {read} revision rows");
    // The vector list gives up to 100 rows: the last page of the ranking ends at 100.
    ensure!(reply["total"] == 100, "{}", reply["total"]);
    let last = store::search::Flags {
        offset: 99,
        ..flags
    };
    let page = store::search::find_with(
        &pool,
        "bot",
        &ws,
        "zzqqxx",
        &filter,
        50,
        Some(&embedder),
        last,
    )
    .await?;
    ensure!(page["items"].as_array().map(Vec::len) == Some(1), "{page}");
    // The control: with all_versions the same search reads revision rows, so the count works.
    let all = store::search::Flags {
        all_versions: true,
        ..flags
    };
    let (_, read) = search(all).await?;
    ensure!(read > 0, "the control search read no revision row");
    ensure!(
        fake.calls.load(std::sync::atomic::Ordering::SeqCst) == 3,
        "provider calls"
    );
    Ok(())
}
