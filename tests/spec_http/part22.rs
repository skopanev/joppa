// Part 22 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_file_version_lists_first_and_both_versions_download() -> Result<()> {
    use anyhow::Context;
    // files-keep-versions-ac1
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["file_target"] = types["consumer"].clone();
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check files on a configured object type"})).await?;
    w.command(json!({"op":"save_object","kind":"file_target","id":"spec","revision":0,"parent":null,"data":{"title":"Specification files","owner":"owner"},"reason":"Create a configured file target"})).await?;
    for address in ["requirement:key", "file_target:spec"] {
        ensure!(w.get(&format!("address={address}")).await?["files"] == json!([]));
        let contents = [
            b"Original specification".as_slice(),
            b"Revised specification".as_slice(),
        ];
        let mut hashes = Vec::new();
        for (id, bytes) in ["a-original", "z-revised"].into_iter().zip(contents) {
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/blobs", w.root, w.ws))
                .bearer_auth(&w.alex)
                .body(bytes.to_vec())
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::OK,
                "{}",
                response.status()
            );
            let blob: Value = response.json().await?;
            let hash = blob["hash"].as_str().unwrap().to_owned();
            w.command_as(&w.alex, json!({"op":"attach","target":address,"id":id,"name":"spec.pdf","content_type":"application/pdf","size":bytes.len(),"hash":hash})).await?;
            hashes.push(hash);
        }
        let object = w.get(&format!("address={address}")).await?;
        let files = object["files"]
            .as_array()
            .context("ordered files list missing")?;
        ensure!(files.len() == 2, "{object}");
        ensure!(
            files[0]["id"] == "z-revised" && files[1]["id"] == "a-original",
            "{files:?}"
        );
        for file in files {
            ensure!(
                file["name"] == "spec.pdf" && file["actor"] == "alex",
                "{file}"
            );
            ensure!(file["created_at"] == file["at"], "{file}");
        }
        let created = |file: &Value| {
            chrono::DateTime::parse_from_str(
                file["created_at"].as_str().unwrap_or(""),
                "%Y-%m-%d %H:%M:%S%.f%#z",
            )
        };
        ensure!(created(&files[0])? > created(&files[1])?, "{files:?}");
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.alex).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":address}}})).send().await?;
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "{}",
            response.status()
        );
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let mcp: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(
            mcp == object,
            "HTTP and MCP returned different file versions"
        );
        for (file, bytes) in files.iter().zip(contents.into_iter().rev()) {
            let response = w
                .http
                .get(format!(
                    "{}/api/workspaces/{}/blobs/{}",
                    w.root,
                    w.ws,
                    file["hash"].as_str().unwrap()
                ))
                .bearer_auth(&w.alex)
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::OK,
                "{}",
                response.status()
            );
            ensure!(response.headers()[reqwest::header::CONTENT_TYPE] == "application/pdf");
            ensure!(
                response.bytes().await?.as_ref() == bytes,
                "A file version changed"
            );
        }
        w.command_as(&w.alex, json!({"op":"detach","target":address,"id":"z-revised","reason":"Hide the revised attachment"})).await?;
        let detached = w.get(&format!("address={address}")).await?;
        ensure!(
            detached["files"].as_array().unwrap().len() == 1
                && detached["files"][0]["id"] == "a-original",
            "{detached}"
        );
        ensure!(detached["annotations"]["attachments"]["z-revised"]["removed"] == true);
        let response = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/blobs/{}",
                w.root, w.ws, hashes[0]
            ))
            .bearer_auth(&w.alex)
            .send()
            .await?;
        ensure!(response.status() == reqwest::StatusCode::OK);
        ensure!(response.bytes().await?.as_ref() == contents[0]);
        let mut data = object["document"]["data"].clone();
        data["title"] = json!(format!("{} revised", data["title"].as_str().unwrap()));
        w.command_as(&w.alex, json!({"op":"save_object","kind":object["kind"],"id":object["id"],"revision":1,"parent":object["document"]["parent"],"data":data,"reason":"Create a later document revision"})).await?;
        let historical = w.get(&format!("address={address}&revision=1")).await?;
        ensure!(
            historical["historical"] == true && historical.get("files").is_none(),
            "A saved document exposes the current files: {historical}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_type_question_is_available_to_an_agent_over_mcp() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    let question = "Which observation changes the current contract?";
    types["observation"] = json!({"title":"Observation","role":"document","root":true,"parents":["*"],"question":question});
    types["record"] = json!({"title":"Record","role":"document","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Configure the authoring question"})).await?;
    let call = |name: &str, arguments: Value| {
        let name = name.to_owned();
        let w = &w;
        async move {
            let reply = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
                .send().await?;
            ensure!(reply.status().is_success(), "{}", reply.status());
            let reply: Value = reply.json().await?;
            ensure!(
                reply.get("error").is_none() && reply["result"]["isError"] != true,
                "{reply}"
            );
            let value: Value =
                serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap())?;
            Ok::<_, anyhow::Error>(value)
        }
    };
    let help = call("joppa_help", json!({"workspace":w.ws})).await?;
    let tree = call("joppa_tree", json!({"workspace":w.ws})).await?;
    ensure!(help["tree"] == tree, "{help} vs {tree}");
    ensure!(
        tree["schema"]["types"]["observation"]["question"] == question,
        "{tree}"
    );
    ensure!(
        tree["schema"]["types"]["record"].get("question").is_none(),
        "{tree}"
    );
    let generic_help = call("joppa_help", json!({})).await?;
    ensure!(
        generic_help.get("tree").is_none() && generic_help["operations"].is_object(),
        "{generic_help}"
    );
    for kind in ["observation", "record"] {
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut command = json!({"op":"save_object","kind":kind,"id":"one","revision":0,"parent":null,"data":{"title":"Agent observation","owner":"owner","description":"An observed failure changes the contract."},"reason":"Record the observation"});
        if kind == "observation" {
            let error = format!(
                "Check that the title and description answer: {question}. Rewrite them if they do not. Then resend with question_checked"
            );
            let conn = w.pool.get().await?;
            let before: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            for checked in [None, Some("A different question")] {
                if let Some(checked) = checked {
                    command["question_checked"] = json!(checked);
                }
                let response = w
                    .http
                    .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                    .bearer_auth(&w.bot)
                    .json(&json!({"request_id":request_id,"command":command}))
                    .send()
                    .await?;
                ensure!(response.status().as_u16() == 422, "{}", response.status());
                ensure!(
                    response.json::<Value>().await?["error"] == error,
                    "wrong question error"
                );
                let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.bot)
                    .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":command}}})).send().await?;
                ensure!(response.status().is_success(), "{}", response.status());
                let refused: Value = response.json().await?;
                ensure!(
                    refused["result"]["isError"] == true
                        && refused["result"]["content"][0]["text"] == error,
                    "{refused}"
                );
            }
            let after: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            ensure!(before == after, "a refused question appended events");
            let revisions: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1 AND address='observation:one'", &[&w.ws]).await?.get(0);
            let receipts: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2", &[&w.ws,&request_id]).await?.get(0);
            ensure!(
                revisions == 0 && receipts == 0,
                "a refused question wrote revisions or a receipt"
            );
            command["question_checked"] = json!(question);
        }
        let saved = call(
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request_id,"command":command}),
        )
        .await?;
        ensure!(saved["object"]["revision"] == 1, "{saved}");
        if kind == "observation" {
            ensure!(saved["object"]["question"] == question, "{saved}");
        } else {
            ensure!(saved["object"].get("question").is_none(), "{saved}");
        }
    }
    Ok(())
}
