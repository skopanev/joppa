//! requirement:embedding-input, the rows that wait for a vector: the partial indexes of both
//! search tables keep only those rows, and the pending query and the vector copy read them by
//! these indexes. The pending query comes from src/search_indexer.rs at build time, so the
//! test plans the statement of the code.
#[path = "common/pg_plan.rs"]
mod pg_plan;
use anyhow::{Result, ensure};
use joppa::embeddings::PROFILE;
use joppa::store::search_indexer::{MAX_ATTEMPTS, REUSE};
use serde_json::json;

const INDEXER: &str = include_str!("../src/search_indexer.rs");
const MIGRATION: &str = include_str!("../migrations/059_search_and_reference_indexes.sql");
const TABLES: [&str; 2] = ["search_documents", "search_revisions"];

/// The condition of a row that waits for a vector of the current profile.
fn waiting(alias: &str) -> String {
    format!("{alias}embedding IS NULL OR {alias}embedding_profile IS DISTINCT FROM $1")
}

// ac:2sjp3vrv
// test:o8ox55ij
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn each_partial_index_keeps_only_the_rows_without_a_current_vector() -> Result<()> {
    let pool = pg_plan::db().await?;
    let conn = pool.get().await?;
    let expected =
        format!("((embedding IS NULL) OR (embedding_profile IS DISTINCT FROM '{PROFILE}'::text))");
    for table in TABLES {
        let index = format!("{table}_waiting");
        let row = conn
            .query_one(
                "SELECT pg_get_expr(i.indpred,i.indrelid) FROM pg_index i WHERE i.indexrelid=('joppa.'||$1)::regclass AND i.indrelid=('joppa.'||$2)::regclass",
                &[&index, &table],
            )
            .await?;
        let predicate: Option<String> = row.get(0);
        println!("{}", json!({"index":index,"predicate":predicate}));
        ensure!(
            predicate.as_deref() == Some(expected.as_str()),
            "{index}: {predicate:?}"
        );
        // The migration names the current profile in the predicate of both indexes.
        let line = MIGRATION
            .lines()
            .find(|l| l.contains(&format!("CREATE INDEX IF NOT EXISTS {index} ")))
            .unwrap_or_default();
        ensure!(
            line.contains(&format!("embedding_profile IS DISTINCT FROM '{PROFILE}'")),
            "the migration line of {index}: {line:?}"
        );
    }
    Ok(())
}

// ac:2sjp3vrv
// test:07bg1g05
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_pending_query_and_the_vector_copy_read_no_table_when_no_row_waits() -> Result<()> {
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "waiting-index").await?;
    pg_plan::many_nodes(&pool, &ws, 2000, 0).await?;
    // Every row of the database gets a current vector: no row waits.
    let conn = pool.get().await?;
    let all: Vec<String> = conn
        .query("SELECT id FROM joppa.workspaces", &[])
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for id in &all {
        pg_plan::vectors(&pool, id).await?;
    }
    pg_plan::analyze(&pool).await?;
    for table in TABLES {
        let counts: (i64, i64) = conn
            .query_one(
                &format!(
                    "SELECT count(*), count(*) FILTER (WHERE {}) FROM joppa.{table}",
                    waiting("")
                ),
                &[&PROFILE],
            )
            .await
            .map(|r| (r.get(0), r.get(1)))?;
        ensure!(counts.0 >= 2000 && counts.1 == 0, "{table}: {counts:?}");
    }
    let pending = pg_plan::literal(INDEXER, "SELECT 'search_documents',workspace")?;
    // The pending query takes each table only under the waiting condition and attempts < 5.
    let condition = format!("({}) AND attempts<$2", waiting(""));
    ensure!(
        pending.matches(&condition).count() == 2,
        "the pending query: {pending}"
    );
    ensure!(MAX_ATTEMPTS == 5, "MAX_ATTEMPTS {MAX_ATTEMPTS}");
    let limit = 64_i64;
    let plan = pg_plan::explain(
        &**conn,
        "FORMAT JSON",
        &pending,
        &[&PROFILE, &MAX_ATTEMPTS, &limit],
    )
    .await?;
    let mut plans = vec![("pending", pending.clone(), plan)];
    for (i, sql) in REUSE.iter().enumerate() {
        ensure!(sql.contains(&waiting("p.")), "copy statement {i}: {sql}");
        let plan = pg_plan::explain(&**conn, "FORMAT JSON", sql, &[&PROFILE]).await?;
        plans.push((TABLES[i], sql.to_string(), plan));
    }
    for (name, sql, plan) in &plans {
        let evidence = json!({"statement":sql,"plan":plan});
        println!("{name}: {evidence}");
        for table in TABLES {
            ensure!(
                pg_plan::seq_scans(plan, table).is_empty(),
                "{name} reads joppa.{table} by a Seq Scan: {evidence}"
            );
        }
        // The waiting rows come from the partial index of each table that the statement takes.
        let names: Vec<String> = pg_plan::nodes(plan)
            .iter()
            .filter_map(|n| n["Index Name"].as_str().map(str::to_owned))
            .collect();
        for table in TABLES.iter().filter(|t| *name == "pending" || *name == **t) {
            ensure!(
                names.contains(&format!("{table}_waiting")),
                "{name} does not read {table}_waiting: {evidence}"
            );
        }
    }
    Ok(())
}
