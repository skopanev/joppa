use deadpool_postgres::Pool;
use joppa::store;
use serde_json::{Value, json};

pub async fn handle(pool: &Pool, actor: &str, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let result: anyhow::Result<Value> = match method {
        "initialize" => {
            let requested = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = match requested {
                "2024-11-05" | "2025-03-26" => requested,
                _ => "2025-06-18",
            };
            Ok(
                json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"joppa","version":env!("CARGO_PKG_VERSION")},"instructions":include_str!("../docs/agent-instructions.txt")}),
            )
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": [
            tool("joppa_list", "List accessible product workspaces", json!({}), json!([])),
            tool("joppa_read", "Read a workspace or resolve a copied globally unique item_id, with history and checks. Supply workspace OR item_id.", json!({"workspace":{"type":"string"},"item_id":{"type":"string"}}), json!([])),
            tool("joppa_command", "Append an operation. Read joppa_help for operation schemas. Actor comes from the authenticated credential, never arguments. Reuse request_id only for identical retries.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"command":{"type":"object"}}), json!(["workspace","request_id","command"])),
            tool("joppa_batch", "Append up to 100 operations atomically. Optional ref labels let later commands refer to generated IDs with {\"$ref\":\"label/id\"}. Read joppa_help first. Retry identical input with the same request_id.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","properties":{"ref":{"type":"string"},"command":{"type":"object"}},"required":["command"],"additionalProperties":false}}}), json!(["workspace","request_id","operations"])),
            tool("joppa_model_docs", "Read Joppa methodology in English: product levels, ownership, readiness, revision history, checks and acceptance. Read before creating a product tree.", json!({"section":{"type":"string","enum":["all","model","process","green"]}}), json!([])),
            tool("joppa_help", "Get all operation shapes and workflow rules before authoring commands", json!({}), json!([]))
        ]})),
        "tools/call" => call(pool, actor, msg)
            .await
            .map(|v| json!({"content":[{"type":"text","text":v.to_string()}]})),
        _ => {
            return Some(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"unknown method"}}),
            );
        }
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(e) => {
            json!({"jsonrpc":"2.0","id":id,"result":{"isError":true,"content":[{"type":"text","text":format!("{e:#}")} ]}})
        }
    })
}
fn tool(name: &str, description: &str, properties: Value, required: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
async fn call(pool: &Pool, actor: &str, msg: &Value) -> anyhow::Result<Value> {
    let a = &msg["params"]["arguments"];
    let field = |k: &str| a[k].as_str().ok_or_else(|| anyhow::anyhow!("missing {k}"));
    match msg["params"]["name"].as_str().unwrap_or("") {
        "joppa_list" => store::list(pool, actor).await,
        "joppa_read" => match (a["workspace"].as_str(), a["item_id"].as_str()) {
            (Some(workspace), None) => store::read(pool, workspace, actor).await,
            (None, Some(id)) => store::read_item(pool, id, actor).await,
            _ => anyhow::bail!("supply workspace OR item_id"),
        },
        "joppa_command" => {
            store::execute_input(
                pool,
                field("workspace")?,
                actor,
                field("request_id")?,
                &a["command"],
            )
            .await
        }
        "joppa_batch" => {
            let operations: Vec<store::BatchOperation> =
                serde_json::from_value(a["operations"].clone())?;
            store::execute_batch(
                pool,
                field("workspace")?,
                actor,
                field("request_id")?,
                &operations,
            )
            .await
        }
        "joppa_model_docs" => {
            let section = a["section"].as_str().unwrap_or("all");
            let docs = [
                ("model", include_str!("../docs/model.md")),
                ("process", include_str!("../docs/process.md")),
                ("green", include_str!("../docs/green.md")),
            ];
            anyhow::ensure!(
                section == "all" || docs.iter().any(|(name, _)| *name == section),
                "unknown documentation section"
            );
            Ok(docs
                .into_iter()
                .filter(|(name, _)| section == "all" || *name == section)
                .map(|(name, text)| (name.to_owned(), json!(text)))
                .collect::<serde_json::Map<String, Value>>()
                .into())
        }
        "joppa_help" => Ok(json!({"operations":include_str!("../docs/api.md")})),
        _ => anyhow::bail!("unknown tool"),
    }
}
