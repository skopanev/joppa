//! Live writes refuse the removed operations over MCP and HTTP and name the replacement.
//! Old journals still hold them; a replay reads them (tests on `common::World`).
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use joppa::removed_ops::{NAMES, refusal};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_removed_operation_is_refused_live_and_names_its_replacement() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let before = w.position().await?;
    let help = w
        .mcp(&w.bot, "joppa_help", json!({"section":"operations"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    for op in NAMES {
        ensure!(help["operations"].get(op).is_none(), "{op} in joppa_help");
        let expected = refusal(op);
        let refused = w.command(&w.bot, json!({"op":op,"id":"x"})).await?;
        ensure!(
            refused
                .as_ref()
                .err()
                .is_some_and(|e| e.contains(&expected)),
            "MCP {op}: {refused:?}"
        );
        let reply = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(&w.bot)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":op,"id":"x"}}))
            .send()
            .await?;
        let status = reply.status();
        let text = reply.text().await?;
        ensure!(
            status.is_client_error() && text.contains(&expected),
            "HTTP {op}: {status} {text}"
        );
    }
    // An old readiness mark with all its fields names the typed field that replaces it.
    let mark = json!({"op":"assess","target":"ac:x","ready":true,"reason":"Clear",
        "expected_position":1});
    let refused = w.command(&w.bot, mark).await?;
    ensure!(
        refused.as_ref().err().is_some_and(|e| e.contains(
            "assess was removed; use save_object with a typed field such as data.stage (fields in set_tree_schema)"
        )),
        "assess: {refused:?}"
    );
    // The command schema has no field of the readiness mark.
    let tools: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .json()
        .await?;
    let command = tools["result"]["tools"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["name"] == "joppa_command"))
        .map(|t| &t["inputSchema"]["properties"]["command"]["properties"]);
    ensure!(
        command.is_some_and(|p| p.is_object()
            && p.get("ready").is_none()
            && p.get("expected_position").is_none()),
        "{tools}"
    );
    // A batch with one removed operation changes nothing.
    let batch = w
        .mcp(
            &w.bot,
            "joppa_batch",
            json!({"request_id":"removed-in-batch","operations":[
                {"command":{"op":"save_object","kind":"requirement","id":"z","revision":0,"parent":null,
                    "data":{"title":"Z","owner":"owner","body":"Rule"},"reason":"Batch test"}},
                {"command":{"op":"revise","req":"z","expected_revision":1}}
            ]}),
        )
        .await?;
    ensure!(
        batch.as_ref().err().is_some_and(
            |e| e.contains("operation 2") && e.contains("revise was removed; use save_object")
        ),
        "{batch:?}"
    );
    ensure!(
        w.position().await? == before,
        "a refused write changed the journal"
    );
    // The help and the command schema name none of them.
    let help: Value = w
        .mcp(&w.bot, "joppa_help", json!({"section":"operations"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    for op in NAMES {
        ensure!(help["operations"].get(op).is_none(), "help names {op}");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_removed_field_is_refused_live_and_names_its_replacement() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = w
        .mcp(&w.bot, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let before = w.position().await?;
    for (command, expected) in [(
        json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
                "status_colors":{"green":"#00aa00"},"reason":"Colour"}),
        "status_colors was removed; use statuses",
    )] {
        let refused = w.command(&w.bot, command.clone()).await?;
        ensure!(
            refused.as_ref().err().is_some_and(|e| e.contains(expected)),
            "{command}: {refused:?}"
        );
    }
    ensure!(
        w.position().await? == before,
        "a refused write changed the journal"
    );
    Ok(())
}
