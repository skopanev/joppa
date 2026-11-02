//! requirement:node-filters over a real server: kind, status, owner, assignee, ready and data
//! keep the same nodes in joppa_objects, joppa_search and their HTTP routes, a bad value is
//! refused, and the filters of owner, assignee and status use their indexes.
#[path = "common/node_filter_world.rs"]
mod node_filter_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use node_filter_world::{kept, seed};
use serde_json::{Value, json};

// node-filters-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_filter_keeps_the_same_nodes_in_the_list_and_the_search() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    seed(&w).await?;
    // ac:a turns green: its only test passes.
    w.recorded(
        &"a".repeat(40),
        json!([{"test":"tests/x.rs > t","status":"pass"}]),
    )
    .await?;
    let under = |extra: Value| {
        let mut filters = json!({"parent":"requirement:r"});
        for (k, v) in extra.as_object().into_iter().flatten() {
            filters[k] = v.clone();
        }
        filters
    };
    for (filters, expected) in [
        (json!({"kind":"rule"}), vec!["rule:x", "rule:z"]),
        (
            json!({"kind":["ac","rule"]}),
            vec!["ac:a", "ac:b", "rule:x", "rule:z"],
        ),
        (json!({"owner":"alex"}), vec!["rule:x"]),
        (json!({"assignee":"alex"}), vec!["ac:b"]),
        (json!({"data":{"tags":["x"]}}), vec!["rule:z"]),
        (under(json!({"status":"green"})), vec!["ac:a"]),
        (
            under(json!({"kind":"ac","owner":"owner"})),
            vec!["ac:a", "ac:b"],
        ),
    ] {
        ensure!(kept(&w, filters.clone()).await? == expected, "{filters}");
    }
    // Each status keeps the nodes that show that status, and a list of two keeps both.
    let all = kept(&w, under(json!({}))).await?;
    let list = w
        .mcp(&w.bot, "joppa_objects", under(json!({})))
        .await?
        .map_err(anyhow::Error::msg)?;
    let mut by_tone = std::collections::BTreeMap::<String, Vec<String>>::new();
    for item in list["items"].as_array().into_iter().flatten() {
        let tone = item["status"].as_str().unwrap_or_default().to_owned();
        by_tone
            .entry(tone)
            .or_default()
            .push(item["address"].as_str().unwrap_or_default().into());
    }
    for (tone, nodes) in &by_tone {
        ensure!(
            kept(&w, under(json!({"status":tone}))).await? == *nodes,
            "{tone}"
        );
    }
    let tones: Vec<&String> = by_tone.keys().collect();
    ensure!(
        kept(&w, under(json!({"status":tones}))).await? == all,
        "{tones:?}"
    );
    // ready: true and ready: false split the subtree; the unassessed ac:a is not ready.
    let ready = kept(&w, under(json!({"ready":true}))).await?;
    let waiting = kept(&w, under(json!({"ready":false}))).await?;
    let mut both = [ready.clone(), waiting.clone()].concat();
    both.sort();
    ensure!(
        both == all && waiting.contains(&"ac:a".to_owned()),
        "{ready:?} {waiting:?}"
    );
    for (args, error) in [
        (
            json!({"status":"blue"}),
            "unknown status blue; use red, purple, yellow, orange, grey, green",
        ),
        (json!({"ready":"yes"}), "ready must be a boolean"),
        (
            json!({"kind":[]}),
            "kind must be a string or a list of strings",
        ),
        (json!({"owner":3}), "owner must be a non-empty string"),
        (json!({"data":"x"}), "data filter must be an object"),
    ] {
        for tool in ["joppa_objects", "joppa_search"] {
            let mut args = args.clone();
            args["query"] = json!("fixture");
            if tool == "joppa_objects" {
                args.as_object_mut().map(|a| a.remove("query"));
            }
            let reply = w.mcp(&w.bot, tool, args.clone()).await?;
            ensure!(reply == Err(error.into()), "{tool} {args}: {reply:?}");
        }
    }
    Ok(())
}

// node-filters-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_assignee_and_status_filters_use_their_indexes() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let mut db = w.pool.get().await?;
    let tx = db.transaction().await?;
    // The test table is small, so a sequential scan is cheaper; the plan must still be able
    // to use each index with the exact expression of the filter clause.
    tx.batch_execute("SET LOCAL enable_seqscan=off").await?;
    for (index, predicate) in [
        ("objects_owner", "o.owner='alex'".to_owned()),
        (
            "objects_assignee",
            format!("{}='alex'", joppa::node_filter::ASSIGNEE),
        ),
        (
            "objects_tone",
            format!("{}='green'", joppa::node_filter::TONE),
        ),
    ] {
        // Only the index of the filter holds its column, so the plan names that index.
        let sql = format!(
            "EXPLAIN SELECT o.address FROM joppa.objects o WHERE o.present AND {predicate}"
        );
        let plan: Vec<String> = tx
            .query(&sql, &[])
            .await?
            .iter()
            .map(|r| r.get(0))
            .collect();
        ensure!(
            plan.iter().any(|line| line.contains(index)),
            "{index}: {plan:?}"
        );
    }
    Ok(())
}
