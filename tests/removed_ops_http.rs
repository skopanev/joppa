//! Live writes refuse the removed operations over MCP and HTTP and name the replacement.
//! Old journals still hold them; a replay reads them (tests on `common::World`).
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use joppa::removed_ops::{NAMES, refusal};
use serde_json::{Value, json};

// A regression check without a criterion: the product spec names no removed operation.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_removed_operation_is_refused_live_and_names_its_replacement() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let before = w.position().await?;
    let help = w
        .mcp(&w.bot, "joppa_help", json!({}))
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
        .mcp(&w.bot, "joppa_help", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    for op in NAMES {
        ensure!(help["operations"].get(op).is_none(), "help names {op}");
    }
    Ok(())
}
