//! requirement:gqqtv5xx over a real server: an agent names itself with `alias` under the
//! account of a person, and every write keeps the alias next to the actor.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use serde_json::{Value, json};

/// One MCP tool call of member O (actor owner) with the session header X-Joppa-Alias. Gives the reply or
/// the refusal text.
async fn with_header(
    w: &World,
    alias: &str,
    tool: &str,
    mut args: Value,
) -> Result<Result<Value, String>> {
    args["workspace"] = json!(w.ws);
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}});
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.owner)
        .header("X-Joppa-Alias", alias)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .context("MCP reply")?;
    Ok(if reply["result"]["isError"] == true {
        Err(text.into())
    } else {
        Ok(serde_json::from_str(text)?)
    })
}

/// One MCP tool call of the owner with the argument `alias`.
async fn aliased(w: &World, tool: &str, mut args: Value, alias: &str) -> Result<Value> {
    args["alias"] = json!(alias);
    w.mcp(&w.owner, tool, args)
        .await?
        .map_err(anyhow::Error::msg)
}

fn save(id: &str, revision: u32, title: &str) -> Value {
    json!({"op":"save_object","kind":"ac","id":id,"revision":revision,"parent":"requirement:r",
        "data":{"title":title,"owner":"owner"},"reason":"Alias check"})
}

/// The revision headers of `address`.
async fn headers(w: &World, address: &str) -> Result<Vec<Value>> {
    let read = w
        .mcp(
            &w.owner,
            "joppa_objects",
            json!({"address":address,"history":true}),
        )
        .await?;
    let read = read.map_err(anyhow::Error::msg)?;
    let items = read["revisions"]
        .as_array()
        .or(read["items"].as_array())
        .context("revisions")?;
    Ok(items.clone())
}

/// The alias of each revision of `address`, from its revision headers.
async fn revisions(w: &World, address: &str) -> Result<Vec<Value>> {
    Ok(headers(w, address)
        .await?
        .iter()
        .map(|r| r.get("alias").cloned().unwrap_or(Value::Null))
        .collect())
}

// ac:ywkh14gw
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_write_keeps_the_alias_next_to_the_actor_and_a_rebuild_keeps_it() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let write =
        |command: Value| json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command});
    aliased(&w, "joppa_command", write(save("b", 0, "B")), "p1").await?;
    // The session header names the alias; the argument wins over it.
    let note = json!({"op":"note","target":"ac:b","id":"n1","text":"Seen"});
    with_header(&w, "p2", "joppa_command", write(note))
        .await?
        .map_err(anyhow::Error::msg)?;
    let mut both = write(save("b", 1, "B two"));
    both["alias"] = json!("p3");
    with_header(&w, "p2", "joppa_command", both)
        .await?
        .map_err(anyhow::Error::msg)?;
    aliased(&w, "joppa_command", write(save("b", 2, "B three")), "p4").await?;
    let results =
        json!({"commit":"a".repeat(40),"results":[{"test":"tests/x.rs > t","status":"pass"}]});
    aliased(&w, "joppa_record_results", results, "runner").await?;
    // The comment page comes only with fields ["comments"].
    let b = w
        .mcp(
            &w.owner,
            "joppa_objects",
            json!({"address":"ac:b","fields":["comments"]}),
        )
        .await?;
    let b = b.map_err(anyhow::Error::msg)?;
    let comment = b["comments"]["items"]
        .as_array()
        .and_then(|c| c.first())
        .cloned()
        .unwrap_or(Value::Null);
    ensure!(
        comment["alias"] == "p2" && comment["actor"] == "owner",
        "comment {b}"
    );
    ensure!(
        revisions(&w, "ac:b").await? == [json!("p1"), json!("p3"), json!("p4")],
        "revisions"
    );
    let history = w
        .mcp(&w.owner, "joppa_history", json!({"order":"newest"}))
        .await?;
    let history = history.map_err(anyhow::Error::msg)?;
    let aliases: Vec<&Value> = history["items"]
        .as_array()
        .context("history")?
        .iter()
        .filter(|i| i["actor"] == "owner")
        .map(|i| &i["alias"])
        .filter(|a| !a.is_null())
        .collect();
    ensure!(
        aliases
            == [
                &json!("runner"),
                &json!("p4"),
                &json!("p3"),
                &json!("p2"),
                &json!("p1")
            ],
        "{history}"
    );
    let entry = history["items"]
        .as_array()
        .context("history")?
        .iter()
        .find(|i| i["alias"] == "p1")
        .context("p1 entry")?["position"]
        .clone();
    let marked = json!({"position":entry,"reason":"Read"});
    let marked = aliased(&w, "joppa_history_mark", marked, "p5").await?;
    ensure!(marked["alias"] == "p5", "{marked}");
    // A rebuild from the journal keeps every alias.
    w.pool
        .get()
        .await?
        .execute(
            "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1",
            &[&w.ws],
        )
        .await?;
    joppa::store::backfill_board(&w.pool).await?;
    ensure!(
        revisions(&w, "ac:b").await? == [json!("p1"), json!("p3"), json!("p4")],
        "revisions after a rebuild"
    );
    Ok(())
}

// ac:ywkh14gw
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_bad_alias_is_refused_and_an_alias_gives_no_access() -> Result<()> {
    let w = record_world::world(&[]).await?;
    // O sends each bad alias.
    for bad in [json!("a b"), json!(""), json!("x".repeat(41)), json!(7)] {
        let args = json!({"request_id":"bad","command":save("c", 0, "C"),"alias":bad});
        let text = match w.mcp(&w.owner, "joppa_command", args).await? {
            Err(text) => text,
            Ok(reply) => anyhow::bail!("{bad} was accepted: {reply}"),
        };
        ensure!(text.contains(joppa::actor_alias::REFUSAL), "{bad}: {text}");
    }
    let text = with_header(&w, "a b", "joppa_list", json!({})).await?;
    ensure!(
        text.is_err_and(|t| t.contains(joppa::actor_alias::REFUSAL)),
        "a bad header"
    );
    // The agent bot stays an agent under any alias: the alias "owner" gives no identity. The
    // save of bot keeps actor bot next to alias owner.
    let args = json!({"request_id":"c","command":save("c", 0, "C"),"alias":"owner"});
    w.mcp(&w.bot, "joppa_command", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    let c = headers(&w, "ac:c").await?;
    ensure!(
        c.len() == 1 && c[0]["actor"] == "bot" && c[0]["alias"] == "owner",
        "the header of ac:c: {c:?}"
    );
    let args = json!({"request_id":"d","command":save("d", 0, "Агент D"),"alias":"агент1"});
    w.mcp(&w.owner, "joppa_command", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        revisions(&w, "ac:d").await? == [json!("агент1")],
        "a Cyrillic alias"
    );
    Ok(())
}
