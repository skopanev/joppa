use deadpool_postgres::Pool;
use joppa::store;
use serde_json::{Value, json};

const WRITING_RULES: &str = include_str!("../docs/writing.md");
const AUTHORING_TEXT: &str = "Prefer English. Follow ASD-STE100 Issue 9 in at least 80% of sentences in each text. Russian is permitted under the same rules. Preserve verbatim user text. Read joppa_help.writing.";

pub async fn handle(
    pool: &Pool,
    actor: &str,
    msg: &Value,
    embedder: Option<&joppa::embeddings::Embedder>,
) -> Option<Value> {
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
                json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"joppa","version":env!("CARGO_PKG_VERSION")},"instructions":format!("{}\n{}", include_str!("../docs/agent-instructions.txt"), WRITING_RULES)}),
            )
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": [
            tool("joppa_list", "List accessible product workspaces", json!({}), json!([])),
            tool("joppa_search", "Find current domains, capabilities, requirements and ACs inside one authorized workspace. Combines full-text and semantic search when embeddings are available; reports coverage. Results are references, not acceptance evidence. Read the matching item before changes.", json!({"workspace":{"type":"string"},"query":{"type":"string","minLength":1,"maxLength":2000},"kind":{"type":"string","enum":["domain","capability","requirement","ac"]},"limit":{"type":"integer","minimum":1,"maximum":50,"default":10}}), json!(["workspace","query"])),
            tool("joppa_read", "Read a workspace index or one requirement's current revision. Supply workspace OR item_id. Every requirement reports revision_count. history=true returns counts only, never events or revision bodies. To read an older revision, explicitly supply revision=N; the response contains exactly one revision and its discussion/evidence. Copied requirement and AC IDs obey the same rules.", json!({"workspace":{"type":"string"},"item_id":{"type":"string"},"req":{"type":"string","description":"One requirement: current revision by default, including its ACs, tasks, runs, issues and revision comments"},"revision":{"type":"integer","minimum":1,"description":"Read exactly this revision of the selected requirement; omit for current. Works with req or a requirement/AC item_id."},"history":{"type":"boolean","default":false,"description":"Discover revision counts without revision bodies or events. Then request one revision by number."},"target":{"type":"string","description":"One object's notes: description, comments and attachments. Address as kind:id, e.g. requirement:r-xxx or ac:r-xxx/2. Cannot be combined with revision or history."}}), json!([])),
            tool("joppa_tasks", "List current Tasks without reading every REQ. available shows tasks this actor can claim, mine shows claimed unfinished tasks, waiting shows unassigned tasks whose AC or contract is not ready, all shows every current Task. Optional filters and ordered priorities use the same selection as claim_next_task. Results are paginated; a cursor expires when the workspace changes.", json!({"workspace":{"type":"string"},"view":{"type":"string","enum":["available","mine","waiting","all"],"default":"available"},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50},"cursor":{"type":"string"},"project":{"type":"string"},"purpose":{"type":"string","enum":["implementation","manual","decision"]},"priorities":{"type":"array","items":{"type":"string"}}}), json!(["workspace"])),
            tool("joppa_command", "Append an operation. Read joppa_help for operation schemas and ASD-STE100 writing rules. Prefer English for new technical text. Actor comes from the authenticated credential. Reuse request_id only for identical retries.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"command":authoring_command_schema()}), json!(["workspace","request_id","command"])),
            tool("joppa_batch", "Append up to 100 operations atomically. Optional ref labels let later commands refer to generated IDs with {\"$ref\":\"label/id\"}. Read joppa_help for schemas and ASD-STE100 writing rules. Prefer English for new technical text. Retry identical input with the same request_id.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","properties":{"ref":{"type":"string"},"command":authoring_command_schema()},"required":["command"],"additionalProperties":false}}}), json!(["workspace","request_id","operations"])),
            tool("joppa_model_docs", "Read Joppa methodology and ASD-STE100 technical writing rules. Prefer English for new technical text. Read before creating a product tree.", json!({"section":{"type":"string","enum":["all","model","process","green","writing"]}}), json!([])),
            tool("joppa_help", "Get operation shapes, workflow rules, and ASD-STE100 technical writing rules. Read before authoring commands.", json!({}), json!([]))
        ]})),
        "tools/call" => call(pool, actor, msg, embedder)
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
fn authoring_command_schema() -> Value {
    let text = || json!({"description":AUTHORING_TEXT});
    json!({
        "type":"object",
        "additionalProperties":true,
        "description":"Operation fields depend on op. Read joppa_help.operations. Apply joppa_help.writing to new technical text.",
        "properties":{
            "title":text(), "body":text(), "text":text(), "description":text(),
            "reason":text(), "contract":text(), "scope":text(), "value":text(),
            "said":{"description":"Verbatim intake. Preserve the original language and wording. ASD-STE100 does not apply."},
            "points":{"description":"Verbatim intake points. Preserve source language and wording. ASD-STE100 does not apply."},
            "acs":{"description":"For revise, AC objects contain text and description governed by the writing rules. For task, entries are AC identifiers.","items":{"properties":{"text":text(),"description":text()}}},
            "assessment":{"properties":{"reason":text()}},
            "checks":{"items":{"properties":{"expected":{"description":AUTHORING_TEXT,"items":text()}}}},
            "evidence":{"description":"For complete_task, use technical text or the required decision comment identifier. For result, observed and note follow the writing rules. Preserve exact facts, logs, code, identifiers, and quotations.","properties":{"observed":{"description":AUTHORING_TEXT,"items":text()},"note":text()}}
        }
    })
}
async fn call(
    pool: &Pool,
    actor: &str,
    msg: &Value,
    embedder: Option<&joppa::embeddings::Embedder>,
) -> anyhow::Result<Value> {
    let a = &msg["params"]["arguments"];
    let field = |k: &str| a[k].as_str().ok_or_else(|| anyhow::anyhow!("missing {k}"));
    match msg["params"]["name"].as_str().unwrap_or("") {
        "joppa_list" => store::list(pool, actor).await,
        "joppa_search" => {
            let limit = match a.get("limit") {
                Some(v) => v
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("limit must be an integer"))?,
                None => 10,
            };
            let kind = match a.get("kind") {
                Some(v) => Some(
                    v.as_str()
                        .ok_or_else(|| anyhow::anyhow!("kind must be a string"))?,
                ),
                None => None,
            };
            store::search::find(
                pool,
                actor,
                field("workspace")?,
                field("query")?,
                kind,
                limit,
                embedder,
            )
            .await
        }
        "joppa_read" => match (a["workspace"].as_str(), a["item_id"].as_str()) {
            (Some(workspace), None) => {
                store::read(pool, workspace, actor, &store::Scope::from_args(a)?).await
            }
            (None, Some(id)) => {
                store::read_item_scoped(pool, id, actor, store::Scope::from_args(a)?).await
            }
            _ => anyhow::bail!("supply workspace OR item_id"),
        },
        "joppa_tasks" => {
            let limit = a
                .get("limit")
                .map(|v| {
                    v.as_u64()
                        .ok_or_else(|| anyhow::anyhow!("limit must be an integer"))
                })
                .transpose()?
                .unwrap_or(50);
            let priorities: Vec<String> = a
                .get("priorities")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .unwrap_or_default();
            store::task_queue_filtered(
                pool,
                field("workspace")?,
                actor,
                a["view"].as_str().unwrap_or("available"),
                usize::try_from(limit)?,
                a["cursor"].as_str(),
                a["project"].as_str(),
                a["purpose"].as_str(),
                &priorities,
            )
            .await
        }
        "joppa_command" => {
            store::execute_remote_input(
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
            store::execute_remote_batch(
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
                ("writing", WRITING_RULES),
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
        "joppa_help" => Ok(
            json!({"operations":include_str!("../docs/api.md"),"search":include_str!("../docs/search.md"),"writing":WRITING_RULES}),
        ),
        _ => anyhow::bail!("unknown tool"),
    }
}
