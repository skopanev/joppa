//! requirement:node-filters over a real server: kind, status, owner, assignee and data
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
        // depth 100 reads the whole subtree of r.
        let mut filters = json!({"parent":"requirement:r","depth":100});
        for (k, v) in extra.as_object().into_iter().flatten() {
            filters[k] = v.clone();
        }
        filters
    };
    let tests = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"ac:a","kind":"test"}),
        )
        .await?;
    let test = tests.map_err(anyhow::Error::msg)?["items"][0]["address"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    for (filters, expected) in [
        (json!({"kind":"rule"}), vec!["rule:x", "rule:z"]),
        (
            json!({"kind":["ac","rule"]}),
            vec!["ac:a", "ac:b", "rule:x", "rule:z"],
        ),
        (json!({"owner":"alex"}), vec!["rule:x"]),
        (json!({"assignee":"alex"}), vec!["ac:b"]),
        (json!({"data":{"tags":["x"]}}), vec!["rule:z"]),
        // The passing test node of ac:a is green too.
        (
            under(json!({"status":"green"})),
            vec!["ac:a", test.as_str()],
        ),
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
    for (args, error) in [
        (
            json!({"status":"gray"}),
            "unknown status gray; use red, purple, yellow, orange, blue, green",
        ),
        (json!({"ready":true}), "the ready filter was removed"),
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
    // A search of the present documents cannot find the tone of archived nodes.
    let args = json!({"query":"fixture","status":"purple"});
    let reply = w.mcp(&w.bot, "joppa_search", args).await?;
    ensure!(reply == Err(ARCHIVED.into()), "{reply:?}");
    // An archived node is purple, and a search with all_versions finds it by that tone.
    let z = w.object("rule:z").await?["revision"].clone();
    let archive =
        json!({"op":"archive_object","target":"rule:z","revision":z,"reason":"Out of scope"});
    w.command(&w.owner, archive)
        .await?
        .map_err(anyhow::Error::msg)?;
    let args = json!({"query":"fixture","all_versions":true,"status":"purple","limit":25});
    let found = w
        .mcp(&w.bot, "joppa_search", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    let mut archived = node_filter_world::sorted(&found["items"], |i| {
        i["address"].as_str().map(str::to_owned)
    });
    archived.dedup();
    ensure!(archived == ["rule:z"], "{found}");
    // The list reads keep the current archived nodes only for the archived tone, also below
    // an archived parent: rule:z is under the archived rule:x.
    let x = w.object("rule:x").await?["revision"].clone();
    let archive =
        json!({"op":"archive_object","target":"rule:x","revision":x,"reason":"Out of scope"});
    w.command(&w.owner, archive)
        .await?
        .map_err(anyhow::Error::msg)?;
    let present = listed(&w, under(json!({}))).await?;
    ensure!(present == ["ac:a", "ac:b", test.as_str()], "{present:?}");
    let purple = listed(&w, under(json!({"status":"purple"}))).await?;
    ensure!(purple == ["rule:x", "rule:z"], "{purple:?}");
    let mut both = [listed(&w, under(json!({"status":"green"}))).await?, purple].concat();
    both.sort();
    let mixed = listed(&w, under(json!({"status":["green","purple"]}))).await?;
    ensure!(mixed == both && mixed.len() > 2, "{mixed:?}");
    let args = json!({"parent":"ac:a","depth":2,"status":"purple","fields":["status","present"]});
    let read = w
        .mcp(&w.bot, "joppa_objects", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    let shown = json!([{"address":"rule:x","status":"purple","present":false},
        {"address":"rule:z","status":"purple","present":false}]);
    ensure!(read["items"] == shown && read["total"] == 2, "{read}");
    Ok(())
}

const ARCHIVED: &str = "status purple marks archived nodes; find them with joppa_objects, or with joppa_search and all_versions";

/// The addresses of a list read, from joppa_objects and from GET /objects, which must agree.
async fn listed(w: &record_world::World, filters: Value) -> Result<Vec<String>> {
    let address = |i: &Value| i["address"].as_str().map(str::to_owned);
    let list = w
        .mcp(&w.bot, "joppa_objects", filters.clone())
        .await?
        .map_err(anyhow::Error::msg)?;
    let pairs = node_filter_world::pairs(&filters);
    let http = node_filter_world::http(w, "objects", &pairs).await?;
    let both = [
        node_filter_world::sorted(&list["items"], address),
        node_filter_world::sorted(&http["items"], address),
    ];
    ensure!(
        both[0] == both[1] && list["total"] == both[0].len(),
        "{filters}: {both:?}"
    );
    Ok(both[0].clone())
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
            "objects_data_assignee",
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
