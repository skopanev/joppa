// Part 18 of tests/spec_http.rs, included there, so the test names stay unchanged.

// signal-in-one-inbox-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_signal_appears_in_the_inbox_list() -> Result<()> {
    let w = world().await?;
    let bot = w.bot.as_str();
    let empty = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    ensure!(
        empty["items"].as_array().is_some_and(Vec::is_empty),
        "{empty}"
    );
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"first","revision":0,"parent":null,"data":{"title":"First","owner":"owner","body":"Refunds are slow"},"reason":"Save"})).await?;
    let inbox = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    let items = inbox["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 1 && items[0]["address"] == "note:first",
        "{inbox}"
    );
    Ok(())
}

// signal-in-one-inbox-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_note_linked_to_any_object_also_appears_in_the_inbox() -> Result<()> {
    let w = world().await?;
    let bot = w.bot.as_str();
    // A note has no parent: it has a related edge to the object that it informs.
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"under","revision":0,"parent":null,"data":{"title":"Under","owner":"owner","body":"About keys"},"relations":{"related":{"edges":[{"to":"requirement:key"}]}},"reason":"Save"}))
        .await?;
    let inbox = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    let items = inbox["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items
            .iter()
            .any(|i| i["address"] == "note:under" && i["parent"].is_null()),
        "{inbox}"
    );
    Ok(())
}

// signal-becomes-spec-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_archives_a_signal_that_needs_no_work_with_a_reason() -> Result<()> {
    let w = world().await?;
    let read = async |address: &str| -> Result<Value> {
        let result = mcp_call(
            &w,
            &w.alex,
            "joppa_objects",
            json!({"workspace":w.ws,"address":address}),
        )
        .await?;
        ensure!(result["isError"] != true, "{result}");
        Ok(serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap_or("null"),
        )?)
    };
    let inbox = async || -> Result<Vec<String>> {
        let list = w.get_as(&w.alex, "kind=note&parent=*&limit=100").await?;
        Ok(list["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|i| i["address"].as_str().map(str::to_owned))
            .collect())
    };
    for mcp in [false, true] {
        let id = if mcp { "mcp-signal" } else { "http-signal" };
        let signal = format!("note:{id}");
        w.note(id, 0, "Dark mode", "Please add dark mode", json!([]))
            .await?;
        ensure!(
            inbox().await?.contains(&signal),
            "mcp={mcp}: not in the Inbox"
        );
        // eve is not a member, so the access layer refuses her and writes nothing.
        refused_without_change(
            &w,
            mcp,
            &w.eve,
            json!({"op":"archive_object","target":signal,"revision":1,"reason":"Out of scope"}),
            "Workspace not found",
        )
        .await?;
        // Each archive check still refuses the member and writes nothing.
        refused_without_change(
            &w,
            mcp,
            &w.alex,
            json!({"op":"archive_object","target":signal,"revision":1,"reason":"  "}),
            "text must not be empty",
        )
        .await?;
        refused_without_change(
            &w,
            mcp,
            &w.alex,
            json!({"op":"archive_object","target":signal,"revision":0,"reason":"Out of scope"}),
            "stale object revision; read the object again",
        )
        .await?;
        ensure!(
            read(&signal).await?.get("present").is_none(),
            "mcp={mcp} {signal}"
        );
        let archive =
            json!({"op":"archive_object","target":signal,"revision":1,"reason":"Out of scope"});
        let reply = send_over(&w, mcp, &w.alex, &archive).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        ensure!(
            !inbox().await?.contains(&signal),
            "mcp={mcp}: still in the Inbox"
        );
        let object = read(&signal).await?;
        ensure!(object["present"] == false, "mcp={mcp}: {object}");
        // The revision headers give the reason of the archive.
        let history = mcp_call(
            &w,
            &w.alex,
            "joppa_objects",
            json!({"workspace":w.ws,"address":signal,"history":true}),
        )
        .await?;
        let history: Value =
            serde_json::from_str(history["content"][0]["text"].as_str().unwrap_or("null"))?;
        ensure!(
            history["items"]
                .as_array()
                .and_then(|items| items.last())
                .is_some_and(|h| h["reason"] == "Out of scope"),
            "mcp={mcp}: {history}"
        );
    }
    Ok(())
}
