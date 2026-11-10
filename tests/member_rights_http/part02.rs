// Part 2 of tests/member_rights_http.rs, included there, so the test names stay unchanged.

const LAST: &str = "the last human member cannot be removed";

// requirement:owner-manages-workspace: Joppa refuses the removal of the last human member.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_removal_of_the_last_human_member_is_refused_over_http_and_mcp() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    let all = json!({"alex":"human","another-member":"human","owner":"human"});
    ensure!(humans(&w).await? == all, "start");
    // carol (another-member) removes herself over MCP.
    let leave = json!({"op":"remove_member","id":"another-member","reason":"Leave W"});
    hidden_lists::ok(&w, &w.other, leave).await?;
    let two = json!({"alex":"human","owner":"human"});
    ensure!(humans(&w).await? == two, "after carol");
    // alex removes bob, the creator of W. A read of W as bob gives 404.
    let bob = json!({"op":"remove_member","id":"owner","reason":"Left the team"});
    let (code, reply) = http_command(&w, &w.alex, bob).await?;
    ensure!(code == 200, "{code} {reply}");
    let one = json!({"alex":"human"});
    ensure!(humans(&w).await? == one, "after bob");
    let read = send(
        &w,
        &Who::Token(&w.owner),
        &format!("/api/workspaces/{}", w.ws),
        None,
    )
    .await?;
    ensure!(read.0 == 404, "the read of bob: {read:?}");
    // alex removes alex over HTTP and over MCP: both are refused.
    let me = json!({"op":"remove_member","id":"alex","reason":"Leave W"});
    let (code, reply) = http_command(&w, &w.alex, me.clone()).await?;
    ensure!(
        code != 200 && reply["error"] == LAST,
        "HTTP: {code} {reply}"
    );
    let text = hidden_lists::refused(&w, &w.alex, me).await?;
    ensure!(text.contains(LAST), "MCP: {text}");
    ensure!(humans(&w).await? == one, "after the refusals");
    // The server replays the journal of W and gives the same member list.
    hidden_lists::rebuild(&mut w).await?;
    ensure!(humans(&w).await? == one, "after the replay");
    let item = access_item(&w, &Who::Token(&w.alex)).await?;
    let ids: Vec<&Value> = item["members"]
        .as_array()
        .context("members")?
        .iter()
        .map(|m| &m["id"])
        .collect();
    ensure!(ids == [&json!("alex")], "{item}");
    Ok(())
}

/// The object, or each object of the list, has no key owner and no key admin. The fields of
/// objects, such as data.owner, are deeper and stay.
fn no_creator_key(value: &Value, path: &str) -> Result<()> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    for item in items {
        if let Some(map) = item.as_object() {
            ensure!(
                !map.contains_key("owner") && !map.contains_key("admin"),
                "{path}: {item}"
            );
        }
    }
    Ok(())
}

// requirement:member-rights: no reply, help text or error text names a workspace owner.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn no_reply_help_or_refusal_names_a_workspace_owner() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let alex = Who::Token(&w.alex);
    access_item(&w, &alex).await?;
    let read = accepted(&w, &alex, &format!("/api/workspaces/{}", w.ws), None).await?;
    for part in ["index", "state"] {
        no_creator_key(&read[part], part)?;
    }
    let list = accepted(&w, &alex, "/api/workspaces", None).await?;
    no_creator_key(&list, "/api/workspaces")?;
    let full = joppa::store::read(&w.pool, &w.ws, "alex", &joppa::store::Scope::full()).await?;
    // The full read is the workspace state itself.
    let state = full.as_object().context("the full read")?;
    ensure!(
        !state.contains_key("owner") && !state.contains_key("admin"),
        "the full read names the creator"
    );
    // data.owner of an object stays.
    ensure!(
        w.object("ac:a").await?["document"]["data"]["owner"] == "owner",
        "data.owner"
    );
    // Each caller of joppa_help is member or human member.
    let help = w
        .mcp(&w.alex, "joppa_help", json!({"section":"operations"}))
        .await?;
    let help = help.map_err(anyhow::Error::msg)?;
    let operations = help["operations"].as_object().context("operations")?;
    ensure!(!operations.is_empty(), "{help}");
    for (name, entry) in operations {
        let callers = entry["callers"].as_array().context("callers")?;
        ensure!(
            callers.iter().all(|c| c == "member" || c == "human member"),
            "{name}: {callers:?}"
        );
    }
    ensure!(!help.to_string().contains("workspace owner"), "{help}");
    // Each refusal of bot names no workspace owner.
    let human = "a human member of the workspace is required";
    let title = "a human member updates the workspace";
    for (command, text) in [
        (json!({"op":"update_workspace","title":"Bot title"}), title),
        (json!({"op":"member","id":"newcomer","role":"human"}), human),
        (
            json!({"op":"remove_member","id":"alex","reason":"Remove"}),
            human,
        ),
        (json!({"op":"delete_workspace","reason":"Close"}), ""),
    ] {
        let refusal = hidden_lists::refused(&w, &w.bot, command.clone()).await?;
        ensure!(refusal.contains(text), "{command}: {refusal}");
        no_owner_words(&refusal)?;
    }
    let path = format!("/api/workspaces/{}/references", w.ws);
    let refs = denied(&w, &Who::Token(&w.bot), &path, None).await?;
    ensure!(
        refs.to_string()
            .contains("a human member of the workspace is required"),
        "{refs}"
    );
    Ok(())
}
