use deadpool_postgres::Pool;
use joppa::{mcp_shapes as shapes, store};
use serde_json::{Value, json};

#[path = "protocol_files.rs"]
pub(crate) mod files;
#[path = "protocol_help.rs"]
mod help;
#[path = "protocol_pages.rs"]
mod pages;
#[path = "protocol_reads.rs"]
mod reads;
#[path = "protocol_schemas.rs"]
mod schemas;
#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
#[path = "protocol_tools.rs"]
mod tools;
pub(crate) use tools::tool;

/// joppa_help section writing gives the writing rules whole.
const WRITING_RULES: &str = include_str!("../docs/writing.md");
/// The instructions of initialize: the read and write rules. The process and writing rules
/// are sections of joppa_help.
const INSTRUCTIONS: &str = include_str!("../docs/agent-instructions.txt");

pub async fn handle(
    pool: &Pool,
    actor: &str,
    msg: &Value,
    embedder: Option<&joppa::embeddings::Embedder>,
    session_alias: Option<&str>,
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
                json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"joppa","version":env!("CARGO_PKG_VERSION")},"instructions":INSTRUCTIONS}),
            )
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::list()})),
        "tools/call" => {
            // The alias leaves the arguments and names the agent of each write.
            let mut msg = msg.clone();
            let args = msg.pointer_mut("/params/arguments");
            match args.map_or(Ok(None), |a| joppa::actor_alias::take(a, session_alias)) {
                Err(e) => Err(e),
                Ok(alias) => {
                    let work = call(pool, actor, &msg, embedder);
                    joppa::actor_alias::scope(alias, work).await
                }
            }
        }
        .and_then(|v| {
            let (name, args) = (&msg["params"]["name"], &msg["params"]["arguments"]);
            let v = joppa::mcp_lean::lean(name.as_str().unwrap_or(""), args, v);
            let text = joppa::mcp_lean::text(&v)?;
            Ok(json!({"content":[{"type":"text","text":text}]}))
        }),
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
async fn call(
    pool: &Pool,
    actor: &str,
    msg: &Value,
    embedder: Option<&joppa::embeddings::Embedder>,
) -> anyhow::Result<Value> {
    let a = &msg["params"]["arguments"];
    let field = |k: &str| a[k].as_str().ok_or_else(|| anyhow::anyhow!("missing {k}"));
    let name = msg["params"]["name"].as_str().unwrap_or("");
    // requirement:read-contract: the compact read shape.
    if name == "joppa_objects" {
        return reads::compact(pool, actor, name, a).await;
    }
    let reply = match name {
        "joppa_list" => pages::list(pool, actor, a).await,
        "joppa_search" => match reads::with_addresses(pool, actor, a, &["parent"]).await {
            Ok(a) => pages::search(pool, actor, &a, embedder).await,
            Err(e) => Err(e),
        },
        "joppa_tree" => {
            store::tree_schema(
                pool,
                field("workspace")?,
                actor,
                a.get("revision")
                    .map(|v| {
                        v.as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .ok_or_else(|| anyhow::anyhow!("invalid schema revision"))
                    })
                    .transpose()?,
            )
            .await
        }
        "joppa_attach" => files::attach(pool, actor, a).await,
        "joppa_attachments" => files::attachments(pool, actor, a).await,
        "joppa_record_results" => store::record_results(pool, field("workspace")?, actor, a).await,
        "joppa_history" => {
            let args = pages::history_args(a)?;
            store::read_history_lean(pool, field("workspace")?, actor, &args).await
        }
        "joppa_history_mark" => store::mark_history(pool, field("workspace")?, actor, a).await,
        "joppa_command" => {
            joppa::commands::reject_actor_field(a)?;
            // MCP hides the admin and UI operations; HTTP and the replay keep them.
            joppa::commands::mcp_admit([&a["command"]], false)?;
            let mut command = a["command"].clone();
            keep_ui_fields(pool, actor, a, [&mut command]).await?;
            store::execute_remote_input(
                pool,
                field("workspace")?,
                actor,
                field("request_id")?,
                &command,
            )
            .await
        }
        "joppa_batch" => {
            joppa::commands::reject_actor_field(a)?;
            if let Some(operations) = a["operations"].as_array() {
                for operation in operations {
                    joppa::commands::reject_actor_field(operation)?;
                }
            }
            let mut operations = a["operations"].clone();
            joppa::batch_defaults::apply(&a["defaults"], &mut operations)?;
            let mut commands: Vec<&mut Value> = vec![];
            for operation in operations.as_array_mut().into_iter().flatten() {
                commands.extend(operation.get_mut("command"));
            }
            keep_ui_fields(pool, actor, a, commands).await?;
            let operations: Vec<store::BatchOperation> = serde_json::from_value(operations)?;
            joppa::commands::mcp_admit(operations.iter().map(|o| &o.command), true)?;
            store::execute_remote_batch(
                pool,
                field("workspace")?,
                actor,
                field("request_id")?,
                &operations,
            )
            .await
        }
        "joppa_help" => help::help(pool, actor, a).await,
        "joppa_tasks" => anyhow::bail!(
            "joppa_tasks was removed; use joppa_objects with kind task, for example {{\"kind\":\"task\",\"assignee\":\"alex\",\"ready\":true}}"
        ),
        "joppa_timeline" => anyhow::bail!(
            "joppa_timeline was removed; use joppa_history, for example {{\"order\":\"newest\",\"after\":0}}"
        ),
        "joppa_tests" => anyhow::bail!(
            "joppa_tests was removed; use joppa_objects with kind test, for example {{\"parent\":\"ac:x\",\"kind\":\"test\"}}"
        ),
        _ => anyhow::bail!("unknown tool"),
    };
    let strip = ["joppa_history", "joppa_search"];
    if strip.contains(&name) {
        let all_workspaces = a["all_workspaces"] == true;
        return reply
            .map(|reply| match name {
                "joppa_history" => shapes::history(reply),
                _ => shapes::search(reply, all_workspaces),
            })
            .map(joppa::read_compact::strip_reply);
    }
    if matches!(name, "joppa_command" | "joppa_batch") {
        return reply.map(shapes::write);
    }
    if name == "joppa_tree" {
        return reply.map(|mut reply| {
            joppa::read_compact::rename_title_length(&mut reply);
            reply
        });
    }
    reply
}

/// A set_tree_schema over MCP keeps the board fields of each type that it omits (ac:iolhwin0).
/// Only a command with set_tree_schema reads the current schema.
async fn keep_ui_fields<'a>(
    pool: &Pool,
    actor: &str,
    a: &Value,
    commands: impl IntoIterator<Item = &'a mut Value>,
) -> anyhow::Result<()> {
    let mut current: Option<Value> = None;
    for command in commands {
        if command["op"] != "set_tree_schema" {
            continue;
        }
        let types = match &current {
            Some(types) => types,
            None => {
                let workspace = a["workspace"].as_str();
                let workspace = workspace.ok_or_else(|| anyhow::anyhow!("missing workspace"))?;
                let tree = store::tree_schema(pool, workspace, actor, None).await?;
                current.insert(tree["schema"]["types"].clone())
            }
        };
        joppa::schema_nodes::keep_ui_fields(command, types);
    }
    Ok(())
}
