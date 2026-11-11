// Part 19 of tests/spec_http.rs, included there, so the test names stay unchanged.

// signal-becomes-spec-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_archives_a_signal_and_keeps_its_related_requirement_link() -> Result<()> {
    let w = world().await?;
    let bot = w.bot.as_str();
    let inbox = async || -> Result<(bool, Value)> {
        let list = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
        let listed = list["items"]
            .as_array()
            .is_some_and(|i| i.iter().any(|i| i["address"] == "note:signal"));
        Ok((listed, list))
    };
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"signal","revision":0,"parent":null,"data":{"title":"Dark mode","owner":"owner","body":"Please add dark mode"},"reason":"Save"})).await?;
    let (listed, list) = inbox().await?;
    ensure!(listed, "{list}");
    w.command_as(bot, json!({"op":"save_object","kind":"requirement","id":"dark","revision":0,"parent":"capability:restrict","data":{"title":"Dark mode","body":"Offer a dark theme","owner":"owner","consumers":["customer"]},"relations":{"related":{"edges":[{"to":"note:signal"}]}},"reason":"Turn the signal into a requirement"})).await?;
    w.command_as(bot, json!({"op":"archive_object","target":"note:signal","revision":1,"reason":"Covered by requirement:dark"})).await?;
    let (listed, list) = inbox().await?;
    ensure!(!listed, "{list}");
    let requirement = object_as(&w, bot, "requirement:dark", None).await?;
    ensure!(
        requirement["relations"]["related"]["edges"]
            .as_array()
            .is_some_and(|l| l.iter().any(|l| l["to"] == "note:signal")),
        "{requirement}"
    );
    let signal = object_as(&w, bot, "note:signal", None).await?;
    // The read counts the incoming edges by group; the relations_of read lists them.
    let group = w
        .get_as(
            bot,
            "relations_of=note:signal&direction=incoming&relation=related&kind=requirement",
        )
        .await?;
    ensure!(
        signal["present"] == false
            // The MCP group of the incoming edges has no direction key.
            && signal["relation_groups"].as_array().is_some_and(|g| g.contains(&json!({"relation":"related","kind":"requirement","count":1})) && g.iter().all(|g| g.get("direction").is_none()))
            && group["items"].as_array().is_some_and(|i| i.iter().any(|i| i["address"] == "requirement:dark")),
        "{signal} {group}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_depends_on_link_shows_on_both_objects_over_http() -> Result<()> {
    // typed-links-ac1
    // A depends_on edge always blocks now; the edge shows on both objects.
    let w = world().await?;
    for id in ["a", "b"] {
        w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":id},"reason":"Save"})).await?;
    }
    w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":"b","revision":1,"parent":null,"data":{"title":"b","owner":"owner","body":"b"},"relations":{"depends_on":{"edges":[{"to":"note:a"}]}},"reason":"Save"})).await?;
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
            document(&b)["relations"]["depends_on"]["edges"][0]["to"] == "note:a",
            "mcp={mcp}: {b}"
        );
        let a = read("a").await?;
        let group = w
            .get_as(
                &w.bot,
                "relations_of=note:a&direction=incoming&relation=depends_on&kind=note",
            )
            .await?;
        // HTTP gives the incoming group with its direction. MCP gives the same group without it.
        let expected = match mcp {
            true => json!({"relation":"depends_on","kind":"note","count":1}),
            false => {
                json!({"direction":"incoming","relation":"depends_on","kind":"note","count":1})
            }
        };
        ensure!(
            a["relation_groups"].as_array().is_some_and(|g| g.contains(&expected))
                && (!mcp || a["relation_groups"].as_array().is_some_and(|g| g.iter().all(|g| g.get("direction").is_none())))
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
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"note","id":"eve","revision":0,"parent":null,"data":{"title":"Signal eve","owner":"owner","body":"Hi"},"reason":"Intake"}}))
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
async fn an_account_without_membership_cannot_save_a_test_node_over_http() -> Result<()> {
    // eve learns nothing about the workspace, and the saved test nodes stay as they were.
    let w = world().await?;
    let list = || async {
        let status = store::test_status(&w.pool, &w.ws, "owner", "ac:key-rights").await?;
        let nodes = w.get("parent=ac:key-rights&kind=test&depth=1").await?;
        Ok::<_, anyhow::Error>((status["tests"].clone(), nodes["items"].clone()))
    };
    let before = list().await?;
    let save = |ws: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/api/workspaces/{ws}/commands", w.root))
                .bearer_auth(&w.eve)
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"test","revision":0,"parent":"ac:key-rights","data":{"title":"a.test.ts > login > ok","owner":"owner"},"reason":"Check refusal"}}))
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
    w.note("leak", 0, "Leak", "the key is sk-123 for prod", json!({}))
        .await?;
    w.note("leak", 1, "Leak", "rotate sk-123 today", json!({}))
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
