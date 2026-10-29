// Part 3 of tests/timeline_http.rs, included there, so the test names stay unchanged.

// workspace-timeline-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_follows_the_cursor_over_250_changes_and_gets_each_one_once_in_journal_order()
-> Result<()> {
    let w = world().await?;
    w.save("bot", "domain:d", 0, None, "Add the domain").await?;
    let p = w.head().await?;
    // Each save changes the body: an unchanged save adds no journal event.
    for revision in 0..250 {
        w.command("bot", json!({"op":"save_object","kind":"signal","id":"s","revision":revision,"parent":null,
            "data":{"title":"Signal S","owner":"pat","body":format!("Source {}", revision + 1)},"reason":"Save the next source"})).await?;
    }
    let journal: Vec<i64> = w
        .pool
        .get()
        .await?
        .query(
            "SELECT position FROM joppa.events WHERE workspace=$1 AND position>$2 ORDER BY position",
            &[&w.ws, &p],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    ensure!(journal.len() == 250, "{}", journal.len());
    let mut positions = vec![];
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let path = match &cursor {
            Some(c) => format!("/timeline?after={p}&cursor={c}"),
            None => format!("/timeline?after={p}"),
        };
        let page = w.get("bot", &path).await?;
        pages += 1;
        ensure!(pages <= 10, "the cursor does not end");
        for item in page["items"].as_array().unwrap() {
            ensure!(item["change"] == "save", "{item}");
            positions.push(item["position"].as_i64().unwrap());
        }
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => {
                ensure!(page["next_cursor"].is_null(), "{page}");
                break;
            }
        }
    }
    ensure!(pages == 5, "{pages} pages of 50");
    ensure!(
        positions.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "a gap or a duplicate: {positions:?}"
    );
    ensure!(positions == journal, "{positions:?} / {journal:?}");
    // MCP follows the same cursor with pages of 25: a limit of 100 is clamped. Each page
    // gives the total of the entries after `after`.
    let mut walked = vec![];
    let mut arguments = json!({"workspace":w.ws,"after":p,"limit":100});
    let pages = journal.len().div_ceil(25);
    for page in 1.. {
        ensure!(page <= pages + 1, "the MCP cursor does not end");
        let reply = w
            .mcp(w.token("bot"), "joppa_timeline", arguments.clone())
            .await?;
        ensure!(reply["total"] == journal.len(), "{reply}");
        walked.extend(
            reply["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["position"].as_i64().unwrap()),
        );
        if reply["next_cursor"].is_null() {
            ensure!(page == pages, "{page} MCP pages of 25");
            break;
        }
        arguments["cursor"] = reply["next_cursor"].clone();
    }
    ensure!(walked == journal, "{walked:?} / {journal:?}");
    Ok(())
}
