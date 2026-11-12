// Part 2 of tests/mcp_many_http.rs, included there, so the test names stay unchanged.

// read-address-fields
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn fields_name_the_keys_of_an_object_read_by_its_address() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    save(
        &w,
        "s",
        "Second rule",
        json!({"related":{"edges":[{"to":"requirement:r"}]}}),
    )
    .await?;
    let fields = json!(["title", "status", "data.owner", "parent", "kind"]);
    let one = ok(&w, json!({"address":"ac:a","fields":fields})).await?;
    let list = ok(&w, json!({"parent":"requirement:r","fields":fields})).await?;
    let mut item = list["items"][0].clone();
    item.as_object_mut().map(|m| m.remove("address"));
    // Only the named paths, with the values of the list item; no revision that was not named.
    ensure!(
        one == item && one["data.owner"] == "owner" && one["parent"] == "requirement:r",
        "{one} {item}"
    );
    let both = ok(
        &w,
        json!({"address":"ac:a,requirement:r","fields":["title"]}),
    )
    .await?;
    ensure!(
        both == json!({"items": [{"address":"ac:a","title":"A"},
            {"address":"requirement:r","title":"R"}]}),
        "{both}"
    );
    // A detail key and a bare document field: relations, status.label, body and an empty key.
    let detail = ok(&w, json!({"address":"requirement:s"})).await?;
    let keys = json!([
        "revision",
        "relations",
        "status.label",
        "body",
        "comments_count",
        "files_count",
        "note"
    ]);
    // A named count gives 0 for an object without comments or files.
    let s = ok(&w, json!({"address":"requirement:s","fields":keys})).await?;
    ensure!(
        s == json!({"revision":detail["revision"],"relations":detail["relations"],
            "status.label":detail["status"]["label"],"body":"Second rule","comments_count":0,
            "files_count":0,"note":null})
            && s["relations"]["related"]["edges"][0]["to"] == "requirement:r",
        "{s} {detail}"
    );
    // Each parameter of the read tools describes itself in the tool schema.
    let tools = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let url = format!("{}/mcp", w.root);
    let tools: Value = w
        .http
        .post(url)
        .bearer_auth(&w.bot)
        .json(&tools)
        .send()
        .await?
        .json()
        .await?;
    let all = tools["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for (name, count) in [("joppa_objects", 23), ("joppa_search", 16)] {
        let tool = all.iter().find(|t| t["name"] == name);
        let properties = tool.map(|t| &t["inputSchema"]["properties"]);
        let described = properties.and_then(Value::as_object).is_some_and(|p| {
            p.len() == count
                && p.values()
                    .all(|v| v["description"].as_str().is_some_and(|d| !d.is_empty()))
        });
        ensure!(described, "{name}: {properties:?}");
    }
    // A list gives the full body, also above 300 characters.
    let long = "A long rule. ".repeat(40);
    save(&w, "long", &long, json!({})).await?;
    let roots = ok(&w, json!({"fields":["body"]})).await?;
    let bodies: Vec<&Value> = roots["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| &i["body"])
        .collect();
    ensure!(
        bodies.contains(&&json!("Second rule")) && bodies.contains(&&json!(long)),
        "{roots}"
    );
    // fields on an older revision give the document of that revision.
    let edit = json!({"op":"save_object","kind":"requirement","id":"s","revision":1,"parent":null,
        "data":{"title":"s","owner":"owner","body":"Second rule, edited"},
        "relations":{"related":{"edges":[{"to":"requirement:r"}]}},"reason":"Edit"});
    w.command(&w.bot, edit).await?.map_err(anyhow::Error::msg)?;
    let old = json!({"address":"requirement:s","revision":1,"fields":["revision","body"]});
    let old = ok(&w, old).await?;
    ensure!(old == json!({"revision":1,"body":"Second rule"}), "{old}");
    let now = ok(
        &w,
        json!({"address":"requirement:s","fields":["revision","body"]}),
    )
    .await?;
    ensure!(
        now == json!({"revision":2,"body":"Second rule, edited"}),
        "{now}"
    );
    // A bare name reads data.<name>, also when the object has no such field.
    let nope = ok(&w, json!({"address":"ac:a","fields":["nope"]})).await?;
    ensure!(nope == json!({"nope":null}), "{nope}");
    for (args, error) in [
        (json!({"fields":["comments"]}), "unknown field comments"),
        (
            json!({"address":"ac:a","fields":["description"]}),
            "use body instead of description; read joppa_help",
        ),
        (
            json!({"fields":["data.contract"]}),
            "use body instead of contract; read joppa_help",
        ),
        (
            json!({"address":"ac:a","fields":["title"],"history":true}),
            "fields reads one revision; history gives the revision headers",
        ),
    ] {
        let reply = read(&w, args.clone()).await?;
        ensure!(reply == Err(error.into()), "{args}: {reply:?}");
    }
    Ok(())
}

// read-reply-ceiling
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_reply_above_256_kb_is_refused_and_one_object_comes_in_full() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let tree = w.mcp(&w.bot, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    let mut types = tree["schema"]["types"].clone();
    types["requirement"]["field_max_length"] = json!(262_144);
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "types":types,"reason":"Long texts"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    let body = "x".repeat(262_144);
    save(&w, "big", &body, json!({})).await?;
    // One object above 256 KB comes in full; a list with it is refused, and fields narrow it.
    let (code, one) = w
        .mcp_raw(
            Some(&w.bot),
            "joppa_objects",
            json!({"workspace":w.ws,"address":"requirement:big"}),
        )
        .await?;
    let text = one["result"]["content"][0]["text"].as_str().unwrap_or("");
    let one: Value = serde_json::from_str(text)?;
    let ceiling =
        "reply exceeds 256 KB; narrow the request with fields, fewer addresses or a smaller limit";
    let both = read(&w, json!({"address":"requirement:big,requirement:r"})).await?;
    let list = read(&w, json!({"fields":["data"]})).await?;
    let narrow = json!({"address":"requirement:big,requirement:r","fields":["title"]});
    let narrow = ok(&w, narrow).await?;
    ensure!(
        code == 200
            && text.len() > 262_144
            && one["data"]["body"] == json!(body)
            && both == Err(ceiling.into())
            && list == Err(ceiling.into())
            && narrow["items"][0]["title"] == "big",
        "{both:?} {list:?} {narrow}"
    );
    Ok(())
}
