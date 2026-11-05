//! requirement:node-filters: the walk below a parent joins each parent key by an index. No
//! step runs a correlated subquery for each pair of a node and a parent.
use anyhow::{Result, ensure};
use joppa::store;

// ac:gm8e0rlr
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_walk_below_a_parent_runs_no_subquery_for_each_node() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let conn = pool.get().await?;
    for parent in [Some("domain:d"), None] {
        let plan: Vec<String> = conn
            .query(
                &format!("EXPLAIN {}", joppa::node_filter::SUBTREE),
                &[&"w", &parent, &None::<i32>, &false],
            )
            .await?
            .iter()
            .map(|row| row.get(0))
            .collect();
        let plan = plan.join("\n");
        ensure!(!plan.contains("SubPlan"), "{plan}");
    }
    Ok(())
}
