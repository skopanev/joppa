// Part 12 of tests/spec_http.rs, included there, so the test names stay unchanged.

// removal-is-archiving-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_object_still_reads_by_address() -> Result<()> {
    let w = world().await?;
    let cap = w.get("address=capability:empty").await?;
    w.command(json!({"op":"archive_object","target":"capability:empty","revision":cap["document"]["revision"],"reason":"Cut"})).await?;
    let read = w.get("address=capability:empty").await?;
    ensure!(read["document"]["present"] == false, "{read}");
    let reply = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?address=capability:empty",
            w.root, w.ws
        ))
        .bearer_auth(&w.bot)
        .send()
        .await?;
    let status = reply.status();
    let body = reply.text().await?;
    ensure!(status == 200, "{status} {body}");
    let read: Value = serde_json::from_str(&body)?;
    ensure!(read["document"]["present"] == false, "{status} {body}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn retired_task_does_not_prevent_archiving_its_parent() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["work"] = json!({"title":"Work","role":"task","status":"none","root":false,"parents":["*"]});
    types["record"] = json!({"title":"Record","role":"document","status":"none","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check archive with configured Task and parent names"})).await?;
    w.command(json!({"op":"save_object","kind":"record","id":"parent","revision":0,"parent":null,"data":{"title":"Parent record","owner":"owner"},"reason":"Create a configured parent"})).await?;
    for (index, (parent, kind)) in [("ac:key-rights", "task"), ("record:parent", "work")]
        .into_iter()
        .enumerate()
    {
        let before = w.get(&format!("address={parent}")).await?;
        let task = format!("{kind}:archive-{index}");
        w.command(json!({"op":"save_object","kind":kind,"id":format!("archive-{index}"),"revision":0,"parent":parent,"data":{"title":"Retired work","owner":"owner","contract":"This work no longer applies","purpose":"implementation","projects":[]},"reason":"Create work under the parent"})).await?;
        let archive = json!({"op":"archive_object","target":parent,"revision":before["current_revision"],"reason":"Remove the unused parent"});
        let error: Value = serde_json::from_str(&w.refused(archive.clone()).await?)?;
        ensure!(
            error["error"] == "cannot archive an object with active children",
            "{error}"
        );
        w.command(json!({"op":"retire_task","id":task,"revision":1,"reason":"The owner no longer needs this work"})).await?;
        let retired = w.get(&format!("address={task}")).await?;
        ensure!(retired["process"]["status"] == "retired", "{retired}");
        // A note has no parent, so a configured document is the present child.
        let note = format!("record:live-child-{index}");
        w.command(json!({"op":"save_object","kind":"record","id":format!("live-child-{index}"),"revision":0,"parent":parent,"data":{"title":"Present input","owner":"owner","body":"This input still applies"},"reason":"Keep an ordinary present child"})).await?;
        let error: Value = serde_json::from_str(&w.refused(archive.clone()).await?)?;
        ensure!(
            error["error"] == "cannot archive an object with active children",
            "{error}"
        );
        w.command(json!({"op":"archive_object","target":note,"revision":1,"reason":"Archive the remaining input"})).await?;
        w.command(archive).await?;
        let after = w.get(&format!("address={parent}")).await?;
        ensure!(after["document"]["present"] == false, "{after}");
        let history = w
            .get(&format!(
                "address={parent}&revision={}",
                before["current_revision"]
            ))
            .await?;
        ensure!(
            history["document"] == before["document"],
            "The parent history changed: {history}"
        );
        let after_task = w.get(&format!("address={task}")).await?;
        ensure!(
            after_task["document"] == retired["document"]
                && after_task["process"]["status"] == "retired",
            "Archive changed the retired Task: {after_task}"
        );
        let list = w
            .get(&format!(
                "parent={}&limit=100",
                before["document"]["parent"].as_str().unwrap_or("*")
            ))
            .await?;
        ensure!(
            list["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["address"] != parent),
            "The archived parent remains in the list: {list}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn done_and_retired_tasks_do_not_block_archiving_a_criterion() -> Result<()> {
    // removal-is-archiving-ac6
    for mcp in [false, true] {
        let w = world().await?;
        w.requirement("arch", "restrict", "Archive rule", "Scope", &[("arch-a", "Criterion A"), ("arch-b", "Criterion B"), ("arch-c", "Criterion C")]).await?;
        w.command_as(&w.bot, json!({"op":"assess","req":"requirement:arch","revision":4,"ac":"ac:arch-a","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
        let done = ready_task(&w, "a-done", "arch-a").await?;
        w.command_as(&w.bot, json!({"op":"claim_task","id":done,"revision":1}))
            .await?;
        w.command_as(&w.bot, json!({"op":"complete_task","commit":"c0ffee0","id":done,"revision":2,"evidence":"Implemented the contract"})).await?;
        let retired = ready_task(&w, "a-retired", "arch-a").await?;
        w.command(json!({"op":"retire_task","id":retired,"revision":1,"reason":"The owner no longer needs this work"})).await?;
        ready_task(&w, "b-open", "arch-b").await?;
        let blocked = ready_task(&w, "c-blocked", "arch-c").await?;
        w.command(json!({"op":"block_task","id":blocked,"revision":1,"reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
        let blocked = w.get(&format!("address={blocked}")).await?;
        ensure!(blocked["process"]["blocked"].is_object(), "{blocked}");
        let tasks_before = [
            w.get(&format!("address={done}")).await?,
            w.get(&format!("address={retired}")).await?,
        ];
        ensure!(
            tasks_before[0]["process"]["status"] == "done"
                && tasks_before[1]["process"]["status"] == "retired",
            "{tasks_before:?}"
        );
        let archive = async |criterion: &str| -> Result<Value> {
            let before = w.get(&format!("address={criterion}")).await?;
            Ok(
                json!({"op":"archive_object","target":criterion,"revision":before["current_revision"],"reason":"Remove the criterion"}),
            )
        };

        let reply = send_over(&w, mcp, &w.owner, &archive("ac:arch-a").await?).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let a = mcp_value(
            &w,
            "joppa_objects",
            json!({"workspace":w.ws,"address":"ac:arch-a"}),
        )
        .await?;
        ensure!(a["present"] == false, "mcp={mcp}: {a}");

        for criterion in ["ac:arch-b", "ac:arch-c"] {
            refused_without_change(
                &w,
                mcp,
                &w.owner,
                archive(criterion).await?,
                "cannot archive an object with active children",
            )
            .await?;
            let after = w.get(&format!("address={criterion}")).await?;
            ensure!(after["document"]["present"] == true, "mcp={mcp}: {after}");
        }

        for before in &tasks_before {
            let after = w
                .get(&format!("address={}", before["address"].as_str().unwrap()))
                .await?;
            ensure!(
                after["document"] == before["document"]
                    && after["process"]["status"] == before["process"]["status"],
                "mcp={mcp}: archive changed {before} -> {after}"
            );
        }
        store::check_journal(&w.pool).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_children_api_returns_one_level_only() -> Result<()> {
    // lazy-children-ac2
    let w = world().await?;
    let list = w.get("limit=50&parent=capability:restrict").await?;
    let kinds: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["kind"].as_str())
        .collect();
    ensure!(kinds == vec!["requirement"], "{kinds:?}");
    let seed =
        json!({"op":"save_object","kind":"capability","id":"wide","revision":0,"parent":"domain:security","data":{"title":"Twenty requirements","owner":"owner"},"reason":"Setup"});
    let seed: Operation = serde_json::from_value(seed)?;
    store::execute(&w.pool, &w.ws, "owner", "wide", &seed).await?;
    let mut expected = std::collections::BTreeSet::new();
    for i in 0..20 {
        let req = format!("wide-{i:02}");
        w.requirement(&req, "wide", &format!("Rule {i}"), "Scope", &[(&format!("{req}-a"), "First check"), (&format!("{req}-b"), "Second check")]).await?;
        expected.insert(format!("requirement:{req}"));
    }
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let reply = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?limit=50&parent=capability:wide",
            w.root, w.ws
        ))
        .bearer_auth(grant["token"].as_str().unwrap())
        .send()
        .await?;
    ensure!(reply.status() == 200, "bot list: {}", reply.text().await?);
    let list: Value = reply.json().await?;
    let items = list["items"].as_array().unwrap();
    let criteria = items.iter().filter(|i| i["kind"] == "ac").count();
    let requirements = items.iter().filter(|i| i["kind"] == "requirement").count();
    let addresses: std::collections::BTreeSet<String> = items
        .iter()
        .filter_map(|i| i["address"].as_str().map(str::to_string))
        .collect();
    ensure!(
        items.len() == 20 && requirements == 20 && criteria == 0 && addresses == expected,
        "{list}"
    );
    Ok(())
}
