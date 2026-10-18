// Part 18 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn inbox_order_uses_the_schema_and_rejects_a_stale_cursor() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["input"] = types["note"].clone();
    types["note"]["inbox"] = json!(false);
    w.command(json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Configure the input type"})).await?;
    for kind in ["input", "note"] {
        for id in ["a-decided", "b-decided", "z-undecided"] {
            let status = if id == "z-undecided" {
                "unprocessed"
            } else {
                "processed"
            };
            w.command(json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":"Input","status":status},"links":[],"reason":"Capture"})).await?;
        }
    }
    w.command(json!({"op":"save_object","kind":"input","id":"y-nested","revision":0,"parent":"domain:security","data":{"title":"Nested input","owner":"owner","body":"Input"},"links":[],"reason":"Capture"})).await?;
    let first = w.get("kind=input&parent=*&limit=1").await?;
    ensure!(first["items"][0]["address"] == "input:y-nested", "{first}");
    let mut cursor = first["next_cursor"].as_str().unwrap().to_owned();
    for address in ["input:z-undecided", "input:a-decided", "input:b-decided"] {
        let page = w
            .get(&format!("kind=input&parent=*&limit=1&cursor={cursor}"))
            .await?;
        ensure!(page["items"][0]["address"] == address, "{page}");
        cursor = page["next_cursor"].as_str().unwrap_or("").into();
    }
    ensure!(cursor.is_empty(), "the last page must finish the list");
    let ordinary = w.get("kind=note&parent=*&limit=100").await?;
    ensure!(
        ordinary["items"][0]["address"] == "note:a-decided"
            && ordinary["items"][0].get("status").is_none(),
        "the type name must not enable queue behavior: {ordinary}"
    );
    let pending = w
        .get("kind=input&parent=*&note_status=unprocessed&limit=100")
        .await?;
    ensure!(pending["items"].as_array().unwrap().len() == 2, "{pending}");
    let processed = w
        .get("kind=input&parent=*&note_status=processed&limit=1")
        .await?;
    let next = w
        .get(&format!(
            "kind=input&parent=*&note_status=processed&limit=1&cursor={}",
            processed["next_cursor"].as_str().unwrap()
        ))
        .await?;
    ensure!(
        processed["items"][0]["address"] == "input:a-decided"
            && next["items"][0]["address"] == "input:b-decided",
        "{processed} / {next}"
    );
    let stale = first["next_cursor"].as_str().unwrap();
    w.command(json!({"op":"set_note_status","target":"input:y-nested","revision":1,"status":"processed","reason":"Reviewed"})).await?;
    let response = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?kind=input&parent=*&limit=1&cursor={stale}",
            w.root, w.ws
        ))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{response:?}"
    );
    ensure!(
        response.json::<Value>().await?["error"]
            == "object list changed; start again without cursor",
        "a status change must expire the cursor"
    );
    let position = w.get("kind=input&parent=*&limit=1").await?["position"]
        .as_u64()
        .unwrap();
    for key in ["2:input:z-undecided", "0:note:a-decided", "0:input:z:bad"] {
        let response = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?kind=input&parent=*&cursor={position}:{key}",
                w.root, w.ws
            ))
            .bearer_auth(&w.owner)
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::CONFLICT,
            "{response:?}"
        );
    }
    let response = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?kind=note&parent=*&cursor={position}:0:input:z-undecided",
            w.root, w.ws
        ))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "a cursor cannot change its order mode"
    );
    Ok(())
}

// signal-in-one-inbox-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_signal_appears_in_the_inbox_list() -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let empty = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    ensure!(
        empty["items"].as_array().is_some_and(Vec::is_empty),
        "{empty}"
    );
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"first","revision":0,"parent":null,"data":{"title":"First","owner":"owner","body":"Refunds are slow"},"links":[],"reason":"Save"})).await?;
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
async fn a_note_under_any_object_also_appears_in_the_inbox() -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"under","revision":0,"parent":"requirement:key","data":{"title":"Under","owner":"owner","body":"About keys"},"links":[],"reason":"Save"}))
        .await?;
    let inbox = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    let items = inbox["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items
            .iter()
            .any(|i| i["address"] == "note:under" && i["parent"] == "requirement:key"),
        "{inbox}"
    );
    Ok(())
}

// signal-becomes-spec-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_archives_a_signal_that_needs_no_work_with_a_reason() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["record"] = json!({"title":"Record","role":"document","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Add a document outside the Inbox"})).await?;
    w.command(json!({"op":"save_object","kind":"record","id":"plain","revision":0,"parent":null,"data":{"title":"Plain record","owner":"owner"},"links":[],"reason":"Save"})).await?;
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
        // A member stays refused for objects outside the Inbox.
        for target in ["capability:empty", "record:plain"] {
            let revision = read(target).await?["document"]["revision"].clone();
            refused_without_change(&w, mcp, &w.alex, json!({"op":"archive_object","target":target,"revision":revision,"reason":"Out of scope"}), "workspace owner or agent required").await?;
            ensure!(
                read(target).await?["document"]["present"] == true,
                "mcp={mcp} {target}"
            );
        }
        // Each other archive check still refuses the member and writes nothing.
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
            read(&signal).await?["document"]["present"] == true,
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
        ensure!(
            object["document"]["present"] == false
                && object["document"]["reason"] == "Out of scope",
            "mcp={mcp}: {object}"
        );
    }
    Ok(())
}
