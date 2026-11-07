//! requirement:8yyywrrq: the data filter of a computed field reads the GIN index of the
//! computed values and the one of the data, so it does not scan each node.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::json;

// ac:ure1a7j8
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_computed_field_filter_reads_the_computed_and_the_data_index() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let mut db = w.pool.get().await?;
    let tx = db.transaction().await?;
    // The test table is small, so a sequential scan is cheaper; the plan must still be able
    // to use each index with the exact expression of the filter clause.
    tx.batch_execute("SET LOCAL enable_seqscan=off").await?;
    let clause = joppa::computed_fields::filter_clause("$1", 1);
    let sql = format!("EXPLAIN SELECT o.address FROM joppa.objects o WHERE o.present{clause}");
    let envelope = json!({"each":[{"stage":"done"}],"computed":{"stage":"done"}}).to_string();
    let plan: Vec<String> = tx
        .query(&sql, &[&envelope])
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for index in ["objects_computed_contains", "objects_data_contains"] {
        ensure!(
            plan.iter().any(|line| line.contains(index)),
            "{index}: {plan:?}"
        );
    }
    Ok(())
}
