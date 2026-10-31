//! requirement:read-contract over a real server: one joppa_objects read gives 1 to 25 objects,
//! `fields` names the keys of an object read by its address, and no list reply is above
//! 256 KB, while one object comes in full.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, args: Value) -> Result<Result<Value, String>> {
    w.mcp(&w.bot, "joppa_objects", args).await
}

async fn ok(w: &record_world::World, args: Value) -> Result<Value> {
    read(w, args).await?.map_err(anyhow::Error::msg)
}

async fn save(w: &record_world::World, id: &str, body: &str, links: Value) -> Result<()> {
    let save = json!({"op":"save_object","kind":"requirement","id":id,"revision":0,
        "parent":null,"data":{"title":id,"owner":"owner","body":body},"links":links,
        "reason":"Add"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    Ok(())
}

// read-many-addresses
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_read_gives_up_to_25_objects_with_their_addresses() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    save(&w, "s", "Second rule", json!([])).await?;
    let r = ok(&w, json!({"address":"requirement:r"})).await?;
    let a = ok(&w, json!({"address":"ac:a"})).await?;
    // One address gives the object itself; more addresses give items in the order of the request.
    let both = ok(&w, json!({"address":"ac:a, requirement:r"})).await?;
    let named = |mut item: Value, address: &str| {
        item["address"] = json!(address);
        item
    };
    ensure!(
        r.get("address").is_none()
            && both
                == json!({"items": [named(a.clone(), "ac:a"), named(r.clone(), "requirement:r")]}),
        "{both}"
    );
    let above = ["requirement:r"; 26].join(",");
    let refused = [
        (json!({"address":above}), "at most 25 addresses in one call"),
        (
            json!({"address":"ac:a,"}),
            "empty address in the address list",
        ),
        (
            json!({"address":"ac:a,requirement:r","history":true}),
            "history, revision and comments_cursor read one address",
        ),
        (
            json!({"address":"ac:a,requirement:r","revision":1}),
            "history, revision and comments_cursor read one address",
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
            && page["items"][24]["address"] == "requirement:s",
        "{page}"
    );
    Ok(())
}

// read-address-fields
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn fields_name_the_keys_of_an_object_read_by_its_address() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    save(&w, "s", "Second rule", json!([{"target":"requirement:r"}])).await?;
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
    // A detail key and a bare document field: links, status.label, body and an empty key.
    let detail = ok(&w, json!({"address":"requirement:s"})).await?;
    let keys = json!([
        "revision",
        "links",
        "status.label",
        "body",
        "comments",
        "note"
    ]);
    let s = ok(&w, json!({"address":"requirement:s","fields":keys})).await?;
    ensure!(
        s == json!({"revision":detail["revision"],"links":detail["links"],
            "status.label":detail["status"]["label"],"body":"Second rule","comments":null,
            "note":null})
            && s["links"][0]["target"] == "requirement:r",
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
    for (name, count) in [
        ("joppa_objects", 17),
        ("joppa_tests", 7),
        ("joppa_search", 7),
    ] {
        let tool = all.iter().find(|t| t["name"] == name);
        let properties = tool.map(|t| &t["inputSchema"]["properties"]);
        let described = properties.and_then(Value::as_object).is_some_and(|p| {
            p.len() == count
                && p.values()
                    .all(|v| v["description"].as_str().is_some_and(|d| !d.is_empty()))
        });
        ensure!(described, "{name}: {properties:?}");
    }
    let roots = ok(&w, json!({"fields":["body"]})).await?;
    ensure!(
        roots["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|i| i["body"] == "Second rule")),
        "{roots}"
    );
    for (args, error) in [
        (
            json!({"address":"ac:a","fields":["nope"]}),
            "unknown field nope",
        ),
        (json!({"fields":["links"]}), "unknown field links"),
        (
            json!({"address":"ac:a","fields":["description"]}),
            "use body instead of description; read joppa_help",
        ),
        (
            json!({"fields":["data.contract"]}),
            "use body instead of contract; read joppa_help",
        ),
        (
            json!({"address":"ac:a","fields":["title"],"revision":1}),
            "fields reads the current revision",
        ),
        (
            json!({"address":"ac:a","fields":["title"],"history":true}),
            "fields reads the current revision",
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
    save(&w, "big", &body, json!([])).await?;
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
