// Part 3 of tests/read_contract_http.rs, included there, so the test names stay unchanged.

/// `count` comments `c00` ... by bot on `target` in one joppa_batch. With `withdrawn`, one more
/// comment `cNN` that bot withdraws after them.
async fn comments(
    w: &record_world::World,
    target: &str,
    count: usize,
    withdrawn: bool,
) -> Result<()> {
    let mut operations: Vec<Value> = (0..count)
        .map(|i| json!({"command":{"op":"note","target":target,"id":format!("c{i:02}"),"text":format!("Comment {i}")}}))
        .collect();
    if withdrawn {
        let id = format!("c{count:02}");
        operations
            .push(json!({"command":{"op":"note","target":target,"id":id,"text":"Asked twice"}}));
        operations.push(json!({"command":{"op":"withdraw_note","target":target,"id":id,"reason":"Asked twice"}}));
    }
    let request_id = format!("read-contract-notes-{}", uuid::Uuid::new_v4().simple());
    let batch = json!({"request_id":request_id,"operations":operations});
    w.mcp(&w.bot, "joppa_batch", batch)
        .await?
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// The keys of an object reply.
fn keys(reply: &Value) -> Vec<&str> {
    reply
        .as_object()
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

// ac:id9sjdvn
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_comment_page_comes_only_on_request() -> Result<()> {
    let w = record_world::world(&[]).await?;
    for id in ["d", "e"] {
        w.command(&w.bot, json!({"op":"save_object","kind":"requirement","id":id,"revision":0,"parent":null,"data":{"title":id.to_uppercase(),"owner":"owner","body":"Rule"},"reason":"Comment page test"})).await?.map_err(anyhow::Error::msg)?;
    }
    comments(&w, "requirement:d", 30, true).await?;
    // e has only a withdrawn comment, so it has no active comment.
    comments(&w, "requirement:e", 0, true).await?;
    let first = json!({"address":"requirement:d","fields":["comments"]});
    let first = read(&w, "joppa_objects", first).await?;
    let page = &first["comments"];
    let items = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        keys(&first) == ["comments"]
            && items.len() == 25
            && page["total"] == 31
            && page["next_cursor"] == "25",
        "{first}"
    );
    let c00 = &items[0];
    ensure!(
        c00["id"] == "c00"
            && c00["actor"] == "bot"
            && ["author", "created_at", "withdrawn"]
                .iter()
                .all(|k| c00.get(*k).is_none()),
        "{c00}"
    );
    // comments_cursor without fields reads as fields ["comments"]: no card key.
    let cursor = json!({"address":"requirement:d","comments_cursor":"25"});
    let cursor = read(&w, "joppa_objects", cursor).await?;
    let page = &cursor["comments"];
    let items = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        keys(&cursor) == ["comments"]
            && items.len() == 6
            && page["total"] == 31
            && page.get("next_cursor").is_none(),
        "{cursor}"
    );
    let last = items.last().cloned().unwrap_or_default();
    ensure!(last["id"] == "c30" && last["withdrawn"] == true, "{last}");
    let both = json!({"address":"requirement:d","fields":["comments"],"comments_cursor":"25"});
    let both = read(&w, "joppa_objects", both).await?;
    ensure!(both == cursor, "{both} {cursor}");
    // The count has no withdrawn comment, and 0 comes when it is named.
    let count = |address: &str| json!({"address":address,"fields":["comments_count"]});
    let d = read(&w, "joppa_objects", count("requirement:d")).await?;
    let e = read(&w, "joppa_objects", count("requirement:e")).await?;
    ensure!(
        d == json!({"comments_count":30}) && e == json!({"comments_count":0}),
        "{d} {e}"
    );
    // A data field comments_count does not change the system field.
    w.command(&w.bot, json!({"op":"save_object","kind":"requirement","id":"d","revision":1,"data":{"comments_count":99},"reason":"Same name as a system field"})).await?.map_err(anyhow::Error::msg)?;
    let d = read(&w, "joppa_objects", count("requirement:d")).await?;
    ensure!(d == json!({"comments_count":30}), "{d}");
    Ok(())
}
