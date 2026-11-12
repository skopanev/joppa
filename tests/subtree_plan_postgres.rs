//! requirement:node-filters: the walk below a parent joins each parent key by an index, and
//! a list below a parent reads its nodes by the primary key, also in a generic plan.
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

// ac:bjxivjms
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_generic_plan_of_a_list_below_a_parent_reads_the_nodes_by_the_primary_key() -> Result<()>
{
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let conn = pool.get().await?;
    let filter = joppa::node_filter::NodeFilter::parse(&serde_json::json!({"parent":"domain:d"}))?;
    let bound = joppa::node_filter::bind(&**conn, "w-plan", &filter, true, false).await?;
    let clause = bound.clause_at([2, 3, 4, 5, 6, 7, 8, 9]);
    // 5000 nodes of one workspace give the planner real statistics. A generic plan does not
    // know the parameters, as a statement after 5 runs on one connection.
    let plan = conn
        .simple_query(&format!(
            "BEGIN;
            INSERT INTO joppa.workspaces(id) VALUES('w-plan');
            INSERT INTO joppa.objects(workspace,address,kind,local_id,revision,parent,present,title,document)
                SELECT 'w-plan','ac:'||n,'ac',n::text,1,'domain:d',true,'A','{{}}'::jsonb FROM generate_series(1,5000) n;
            ANALYZE joppa.objects;
            SET LOCAL plan_cache_mode=force_generic_plan;
            PREPARE list_plan(text,text,text[],boolean,text[],text[],text,text,text) AS
                SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND {clause};
            EXPLAIN EXECUTE list_plan('w-plan','w-plan',ARRAY['ac:1'],false,NULL,NULL,NULL,NULL,NULL);
            DEALLOCATE list_plan; ROLLBACK;"
        ))
        .await?;
    let plan: Vec<&str> = plan
        .iter()
        .filter_map(|m| match m {
            tokio_postgres::SimpleQueryMessage::Row(row) => row.get(0),
            _ => None,
        })
        .collect();
    let plan = plan.join("\n");
    ensure!(
        plan.contains("objects_pkey") && plan.contains("address = ANY"),
        "{plan}"
    );
    ensure!(
        plan.lines()
            .any(|l| l.contains("Index Cond") && l.contains("address = ANY")),
        "{plan}"
    );
    Ok(())
}
