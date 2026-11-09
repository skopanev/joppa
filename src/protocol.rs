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

const WRITING_RULES: &str = include_str!("../docs/writing.md");
/// The authoring rules of the text fields, once in the description of joppa_command.
const AUTHORING_TEXT: &str = "Text fields (body, text, reason, value and data values): each value holds at most the field_max_length of its type (default 1024 characters). Over 1 kb: prefer attach as a file. Prefer English. Follow ASD-STE100 Issue 9 in at least 80% of sentences in each text. Russian is permitted under the same rules. Preserve verbatim user text. Use clean Markdown, and write an object reference as [Title](kind:id), never a bare ID. Do not put link lists or progress notes in text fields: use links and the note op. Read joppa_help with section writing.";

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
                json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"joppa","version":env!("CARGO_PKG_VERSION")},"instructions":format!("{}\n{}", include_str!("../docs/agent-instructions.txt"), WRITING_RULES)}),
            )
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": json!([
            tool("joppa_list", "List your workspaces: items {id, title, hidden}. hidden: true means you hid the workspace from your web lists; MCP reads and writes it as usual.", json!({"cursor":{"type":"string"}}), json!([])),
            tool("joppa_search", "Find objects by words and meaning. A result gives address, title, revision, score and excerpt. Read the object before a change. A list reply over 256 KB is refused.", schemas::search(), json!(["workspace","query"])),
            tool("joppa_tree", "Read the workspace schema: types with parents, roles, status rules (own, children, links, none), questions, references, typed data fields, filters and test_results. A typed field names type (text, number, boolean, enum, list), values and from. The order of values is the rollup priority: the first value wins. from: own (default, the node stores it), children, links (both directions) or parent; the server computes the other three. An enum takes children or links. A list field or a multiple reference with from children gives the sorted union of the values of the children. A filter field that a result field of type enum or ref also names is a dimension: a data filter with its value, such as {\"env\":\"dev\"}, gives the tones of that value. Change it with set_tree_schema and send schema.revision; put_types and remove_types change single types. A type can set field_max_length for its data values. The reply gives field_hint.", json!({"workspace":{"type":"string"},"revision":{"type":"integer","minimum":0}}), json!(["workspace"])),
            tool("joppa_objects", "Read objects. Without address: a page of items {address, title, status tone, assignee when set}. parent gives the direct children; depth up to 100 reads deeper; parent \"*\" reads every level. fields keeps the named keys: [\"children\"] counts children, [\"links\"] gives links. With address: one object with revision, parent, data, status, process, comments and links; 2 to 25 addresses give {items}. A list reply over 256 KB is refused.", schemas::objects(), json!([])),
            tool("joppa_record_results", "Record the test results of one commit: commit (a lowercase git sha) and results [{test, status, ...fields}]. Name each test by the address of its test node, such as test:<id>, or by its public item ID: it binds that one node. A title binds every present test node with that title or with that earlier title. The server stores the result under the address of each bound node. The test_results block of joppa_tree gives the status names and fields. The latest record of a test decides it. The server sets actor and time. A call with actor or at is refused. The reply gives unbound: the tests with no present test node. The server keeps their results under the given name, so a test node with that title counts them later. unbound does not check under which criterion a test node stands. A full-run record needs unbound: [] before a merge.", json!({"workspace":{"type":"string"},"commit":{"type":"string"},"results":{"type":"array","minItems":1,"items":{"type":"object","properties":{"test":{"type":"string","description":"test:<id> or the public item ID of one test node, or a title that binds every present test node with it"},"status":{"type":"string","description":"a name from the tree schema test_results.statuses"}},"required":["test","status"],"additionalProperties":true}}}), json!(["workspace","commit","results"])),
            tool("joppa_history", "Read the History of one workspace: one entry for each recorded change. An entry gives position, at, actor, alias, address, title, revision, change, op, reason and status: open until a member marks it processed, then processed with its mark. order newest reads the newest first; after reads only the entries after a position; open=true gives only the open entries, and next_item names the oldest open entry.", json!({"workspace":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":25,"default":25},"cursor":{"type":"string"},"order":{"type":"string","enum":["oldest","newest"],"default":"oldest"},"after":{"type":"integer","minimum":0,"default":0},"open":{"type":"boolean","default":false}}), json!(["workspace"])),
            tool("joppa_history_mark", "Mark one open History entry processed: position, a short reason and at most 15 nodes that the work created or changed. The server sets actor and time; a call with actor or at is refused.", json!({"workspace":{"type":"string"},"position":{"type":"integer","minimum":1},"reason":{"type":"string"},"nodes":{"type":"array","maxItems":15,"items":{"type":"string"}}}), json!(["workspace","position","reason"])),
            tool("joppa_command", &["Append one operation. Read joppa_help with op for its fields. To update, send the revision you read and only the changed data fields: data merges by key, null removes a field, and an omitted parent or links stay. clean: true replaces data whole. Link objects only with the links array of save_object. Add findings as comments: {\"op\":\"note\",\"target\":\"kind:id\",\"id\":\"<unique>\",\"text\":\"...\"}. A note id has 1 to 120 ASCII letters, digits, \"-\", \"_\" or \".\". A test is a node: save_object {kind: \"test\", parent: \"ac:x\", data: {title: \"<the scenario in business language>\", owner}}. Reuse request_id only for an identical retry.", AUTHORING_TEXT].join(" "), json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"command":authoring_command_schema()}), json!(["workspace","request_id","command"])),
            tool("joppa_batch", "Append up to 100 operations atomically, by the rules of joppa_command. A ref label lets a later command name a generated ID as {\"$ref\":\"label/id\"} or a saved object as {\"$ref\":\"label/address\"}. defaults gives save_object fields to each save_object that omits them. A note id has 1 to 120 ASCII letters, digits, \"-\", \"_\" or \".\". Retry identical input with the same request_id.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"defaults":{"type":"object","description":"save_object fields for each save_object of the batch that omits them, for example {\"reason\":\"...\",\"data\":{\"owner\":\"...\"}}. No op."},"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","properties":{"ref":{"type":"string"},"command":{"type":"object","description":"One operation object, by the rules of joppa_command.command."}},"required":["command"],"additionalProperties":false}}}), json!(["workspace","request_id","operations"])),
            tool("joppa_help", "Read operation shapes and writing rules. Without section or op: a table of contents. section: operations, writing, objects, search, tree or examples; tree and examples need workspace. op: the fields and callers of one operation. The reply gives field_hint.", json!({"workspace":{"type":"string"},"section":{"type":"string","enum":["operations","writing","objects","search","tree","examples"],"description":"One section of the help."},"op":{"type":"string","description":"One operation name: the reply gives its required fields and callers."}}), json!([]))
        ]).as_array().into_iter().flatten().chain(files::tools().iter())
            .cloned().collect::<Vec<_>>()})),
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
/// One tool. Every tool takes `alias`: the name of the agent that writes under the account of
/// the session.
pub(crate) fn tool(name: &str, description: &str, mut properties: Value, required: Value) -> Value {
    properties["alias"] = json!({"type":"string","description":"Optional. Your agent name under this account, such as p1. The journal shows it next to the actor."});
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
fn authoring_command_schema() -> Value {
    let text =
        || json!({"description":"Text by the text rules in the description of joppa_command."});
    json!({
        "type":"object",
        "additionalProperties":true,
        "description":"Operation fields depend on op. Read joppa_help with section operations. Apply joppa_help section writing to new technical text. Each field that names an object of the workspace takes its address or its public item ID, such as joppa-R-DP6KXM62: target, parent, duplicate_of, links[].target, the reference fields of data and the id of save_object. The journal stores the kind:id address.",
        "properties":{
            "op":{"type":"string","enum":joppa::commands::OPERATIONS.iter().map(|o| o.name).collect::<Vec<_>>(),"description":"Operation name. Read joppa_help with this op for its required fields and callers."},
            "revision":{"description":"For save_object, archive_object, restore_object and set_tree_schema, send the revision you read. Zero creates a new object or changes the default schema. The server creates the next revision."},
            "clean":{"type":"boolean","description":"For save_object with revision > 0: true replaces data whole. Without it, data merges by key, null removes a field, and an omitted parent or links stay."},
            "data":{"type":"object","description":"Document fields. data.owner: the member ID of a human owner, required on every save; an AC without it takes the owner of its REQ. data.title: one concise statement (max 128 chars). data.body: the text of every object type; data.description, data.contract and data.decision_required are refused. Each other text value follows the text rules of joppa_command. A computed field (from children, links or parent) is refused: the server sets it."},
            "title":{"description":"One concise statement (max 128 chars). Prefer English. Follow ASD-STE100 Issue 9."}, "body":text(), "text":text(),
            "reason":text(), "value":text(),
            "hidden":{"type":"boolean","description":"For update_workspace: true hides the workspace from your own web lists, false shows it again. Any member may send it."}
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
            store::read_history(pool, field("workspace")?, actor, &pages::history_args(a)?).await
        }
        "joppa_history_mark" => store::mark_history(pool, field("workspace")?, actor, a).await,
        "joppa_command" => {
            joppa::commands::reject_actor_field(a)?;
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
            joppa::commands::reject_actor_field(a)?;
            if let Some(operations) = a["operations"].as_array() {
                for operation in operations {
                    joppa::commands::reject_actor_field(operation)?;
                }
            }
            let mut operations = a["operations"].clone();
            joppa::batch_defaults::apply(&a["defaults"], &mut operations)?;
            let operations: Vec<store::BatchOperation> = serde_json::from_value(operations)?;
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
    if matches!(name, "joppa_tree" | "joppa_help") {
        return reply.map(|mut reply| {
            joppa::read_compact::rename_title_length(&mut reply);
            // The tree inside joppa_help equals the joppa_tree reply.
            reply["field_hint"] = json!(joppa::field_limit::HINT);
            if let Some(tree) = reply.get_mut("tree") {
                tree["field_hint"] = json!(joppa::field_limit::HINT);
            }
            reply
        });
    }
    reply
}
