// Part 1 of tests/mcp_many_http.rs, included there, so the test names stay unchanged.

/// The public item ID of `kind:id` in the workspace of `w`.
async fn public_id(w: &record_world::World, kind: &str, id: &str) -> Result<String> {
    let sql = "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind=$2 AND local_id=$3";
    let conn = w.pool.get().await?;
    Ok(conn.query_one(sql, &[&w.ws, &kind, &id]).await?.get(0))
}

// read-many-addresses
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_read_gives_up_to_25_objects_with_their_addresses() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    save(&w, "s", "Second rule", json!({})).await?;
    // ac:a has 2 comments and 1 file, the public item ID P and the alias ac:old.
    let notes: Vec<Value> = (0..2)
        .map(
            |i| json!({"command":{"op":"note","target":"ac:a","id":format!("n{i}"),"text":"Seen"}}),
        )
        .collect();
    let batch = json!({"request_id":"many-notes","operations":notes});
    w.mcp(&w.bot, "joppa_batch", batch)
        .await?
        .map_err(anyhow::Error::msg)?;
    let file = json!({"target":"ac:a","filename":"e.txt","content":"Evidence"});
    w.mcp(&w.bot, "joppa_attach", file)
        .await?
        .map_err(anyhow::Error::msg)?;
    let p = public_id(&w, "ac", "a").await?;
    let alias =
        "INSERT INTO joppa.object_aliases(workspace,address,target) VALUES ($1,'ac:old','ac:a')";
    w.pool.get().await?.execute(alias, &[&w.ws]).await?;
    let r = ok(&w, json!({"address":"requirement:r"})).await?;
    let s = ok(&w, json!({"address":"requirement:s"})).await?;
    let a = ok(&w, json!({"address":"ac:a"})).await?;
    // The card counts the comments and the files, and lists neither.
    ensure!(
        a["comments_count"] == 2
            && a["files_count"] == 1
            && a.get("comments").is_none()
            && a.get("files").is_none(),
        "{a}"
    );
    // One address gives the object itself; more addresses give items in the order of the request.
    let named = |item: &Value, address: &str| {
        let mut item = item.clone();
        item["address"] = json!(address);
        item
    };
    let both = ok(&w, json!({"address":"ac:a, requirement:r"})).await?;
    ensure!(
        r.get("address").is_none()
            && both == json!({"items": [named(&a, "ac:a"), named(&r, "requirement:r")]}),
        "{both}"
    );
    // A duplicate gives 2 equal items. The alias and the public item ID give ac:a.
    let twice = ok(&w, json!({"address":"ac:a,ac:a"})).await?;
    ensure!(
        twice == json!({"items": [named(&a, "ac:a"), named(&a, "ac:a")]}),
        "{twice}"
    );
    let other = ok(&w, json!({"address":format!("ac:old,{p},requirement:s")})).await?;
    ensure!(
        other
            == json!({"items": [named(&a, "ac:a"), named(&a, "ac:a"),
                named(&s, "requirement:s")]}),
        "{other}"
    );
    let above = ["requirement:r"; 26].join(",");
    let one_address = "history, revision and comments_cursor read one address";
    let refused = [
        (json!({"address":above}), "at most 25 addresses in one call"),
        (
            json!({"address":"ac:a,"}),
            "empty address in the address list",
        ),
        (
            json!({"address":"ac:a,requirement:r","history":true}),
            one_address,
        ),
        (
            json!({"address":"ac:a,requirement:r","revision":1}),
            one_address,
        ),
        (
            json!({"address":"ac:a,requirement:r","comments_cursor":"25"}),
            one_address,
        ),
        (json!({"address":"ac:a,nope:x"}), "unknown object"),
    ];
    for (args, error) in refused {
        let reply = read(&w, args.clone()).await?;
        ensure!(reply == Err(error.into()), "{args}: {reply:?}");
    }
    let page = ["requirement:s"; 25].join(",");
    let page = ok(&w, json!({"address":page})).await?;
    ensure!(
        page["items"].as_array().map(Vec::len) == Some(25)
            && page["items"][24] == named(&s, "requirement:s"),
        "{page}"
    );
    Ok(())
}
