// Part 19 of tests/spec_http.rs, included there, so the test names stay unchanged.

// signal-becomes-spec-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_archives_a_signal_and_keeps_its_related_requirement_link() -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let inbox = async || -> Result<(bool, Value)> {
        let list = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
        let listed = list["items"]
            .as_array()
            .is_some_and(|i| i.iter().any(|i| i["address"] == "note:signal"));
        Ok((listed, list))
    };
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"signal","revision":0,"parent":null,"data":{"title":"Dark mode","owner":"owner","body":"Please add dark mode"},"links":[],"reason":"Save"})).await?;
    let (listed, list) = inbox().await?;
    ensure!(listed, "{list}");
    w.command_as(bot, json!({"op":"save_object","kind":"requirement","id":"dark","revision":0,"parent":"capability:restrict","data":{"title":"Dark mode","body":"Offer a dark theme","owner":"owner","consumers":["customer"]},"links":[{"relation":"related","target":"note:signal"}],"reason":"Turn the signal into a requirement"})).await?;
    w.command_as(bot, json!({"op":"archive_object","target":"note:signal","revision":1,"reason":"Covered by requirement:dark"})).await?;
    let (listed, list) = inbox().await?;
    ensure!(!listed, "{list}");
    let requirement = object_as(&w, bot, "requirement:dark", None).await?;
    ensure!(
        requirement["links"]["items"].as_array().is_some_and(|l| l
            .iter()
            .any(|l| l["link"]["relation"] == "related" && l["link"]["target"] == "note:signal")),
        "{requirement}"
    );
    let signal = object_as(&w, bot, "note:signal", None).await?;
    // The read counts the incoming links by group; the group read lists them.
    let group = w
        .get_as(
            bot,
            "links_of=note:signal&direction=incoming&relation=related&kind=requirement",
        )
        .await?;
    ensure!(
        signal["document"]["present"] == false
            && signal["link_groups"].as_array().is_some_and(|g| g.iter().any(|g| g["direction"] == "incoming" && g["relation"] == "related" && g["kind"] == "requirement" && g["count"] == 1))
            && group["items"].as_array().is_some_and(|i| i.iter().any(|i| i["address"] == "requirement:dark")),
        "{signal} {group}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_depends_on_link_shows_on_both_objects_over_http() -> Result<()> {
    // typed-links-ac1
    let w = world().await?;
    for id in ["a", "b"] {
        w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":id},"links":[],"reason":"Save"})).await?;
    }
    w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":"b","revision":1,"parent":null,"data":{"title":"b","owner":"owner","body":"b"},"links":[{"target":"note:a","relation":"depends_on"}],"reason":"Save"})).await?;
    let (actor, command) = last_event(&w).await?;
    ensure!(actor == "bot", "{actor}: {command}");
    for mcp in [false, true] {
        let read = async |id: &str| -> Result<Value> {
            if mcp {
                object_as(&w, &w.bot, &format!("note:{id}"), None).await
            } else {
                w.get_as(&w.bot, &format!("address=note:{id}")).await
            }
        };
        let b = read("b").await?;
        ensure!(
            b["document"]["links"][0]["relation"] == "depends_on"
                && b["document"]["links"][0]["target"] == "note:a",
            "mcp={mcp}: {b}"
        );
        let a = read("a").await?;
        let group = w
            .get_as(
                &w.bot,
                "links_of=note:a&direction=incoming&relation=depends_on&kind=note",
            )
            .await?;
        ensure!(
            a["link_groups"].as_array().is_some_and(|g| g.iter().any(|g| g["direction"] == "incoming" && g["relation"] == "depends_on" && g["kind"] == "note" && g["count"] == 1))
                && group["items"].as_array().is_some_and(|i| i.iter().any(|i| i["address"] == "note:b")),
            "mcp={mcp}: {a} {group}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_add_a_signal_over_http() -> Result<()> {
    // signal-in-one-inbox-ac3: eve learns nothing, not even that the workspace exists.
    let w = world().await?;
    let save = |ws: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/api/workspaces/{ws}/commands", w.root))
                .bearer_auth(&w.eve)
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"note","id":"eve","revision":0,"parent":null,"data":{"title":"Signal eve","owner":"owner","body":"Hi"},"links":[],"reason":"Intake"}}))
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let member_only = save(w.ws.clone()).await?;
    let absent = save(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(
        member_only.0 == 404 && member_only == absent,
        "{member_only:?} vs {absent:?}"
    );
    ensure!(
        member_only.1.contains("Workspace not found"),
        "{member_only:?}"
    );
    let inbox = w.get("kind=note&parent=*&limit=100").await?;
    ensure!(
        inbox["items"].as_array().is_some_and(|i| i.is_empty()),
        "{inbox}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_save_a_test_list_over_http() -> Result<()> {
    // eve learns nothing about the workspace, and the saved test list stays as it was.
    let w = world().await?;
    let list = || async {
        let status: Value = w
            .http
            .get(format!("{}/api/workspaces/{}/tests", w.root, w.ws))
            .bearer_auth(&w.owner)
            .query(&[("ac", "ac:key-rights")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok::<_, anyhow::Error>((status["tests"].clone(), status["tests_saved_by"].clone()))
    };
    let before = list().await?;
    let save = |ws: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/api/workspaces/{ws}/commands", w.root))
                .bearer_auth(&w.eve)
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"set_ac_tests","target":"ac:key-rights","tests":["a.test.ts > login > ok"],"reason":"Check refusal"}}))
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let member_only = save(w.ws.clone()).await?;
    let absent = save(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(
        member_only.0 == 404 && absent.0 == 404,
        "{member_only:?} vs {absent:?}"
    );
    ensure!(
        member_only.1.contains("Workspace not found") && absent.1.contains("Workspace not found"),
        "{member_only:?} vs {absent:?}"
    );
    ensure!(member_only.1 == absent.1, "{member_only:?} vs {absent:?}");
    let after = list().await?;
    ensure!(after == before, "{before:?} vs {after:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_redacted_fragment_cannot_be_read_anywhere() -> Result<()> {
    // owner-redacts-secret-ac1
    let w = world().await?;
    w.note("leak", 0, "Leak", "the key is sk-123 for prod", json!([]))
        .await?;
    w.note("leak", 1, "Leak", "rotate sk-123 today", json!([]))
        .await?;
    ensure!(
        w.search("sk-123").await? == ["note:leak"],
        "search must find the fragment before the redaction"
    );
    w.command(json!({"op":"redact_object","target":"note:leak","fragment":"sk-123","reason":"leaked key"}))
        .await?;
    let reads = [
        w.get("address=note:leak").await?,
        w.get("address=note:leak&revision=1").await?,
        w.get("address=note:leak&revision=2").await?,
        w.get("address=note:leak&history=true").await?,
        w.get("kind=note&parent=*&limit=100").await?,
    ];
    for read in &reads {
        ensure!(!read.to_string().contains("sk-123"), "{read}");
    }
    ensure!(
        reads[1]["document"]["data"]["body"] == "the key is [redacted] for prod",
        "{}",
        reads[1]
    );
    let found = w.search("sk-123").await?;
    ensure!(found.is_empty(), "{found:?}");
    let state: String = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT value::text FROM joppa.state_entries WHERE workspace=$1 AND section='redactions' AND key='note:leak'",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(
        state.contains("leaked key") && state.contains("owner") && !state.contains("sk-123"),
        "{state}"
    );
    // A rebuild from the journal replays the redaction and agrees with the stored revisions.
    w.pool
        .get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::backfill_board(&w.pool).await?;
    let rebuilt = w.get("address=note:leak&revision=1").await?;
    ensure!(
        rebuilt["document"]["data"]["body"] == "the key is [redacted] for prod",
        "{rebuilt}"
    );
    Ok(())
}
