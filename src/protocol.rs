use deadpool_postgres::Pool;
use joppa::{mcp_shapes as shapes, store};
use serde_json::{Value, json};

#[path = "protocol_files.rs"]
mod files;
#[path = "protocol_help.rs"]
mod help;
#[path = "protocol_pages.rs"]
mod pages;
#[path = "protocol_reads.rs"]
mod reads;
#[path = "protocol_schemas.rs"]
mod schemas;

const WRITING_RULES: &str = include_str!("../docs/writing.md");
const AUTHORING_TEXT: &str = "Concise text (max field_max_length of the type, default 1024 chars). Over 1 kb: prefer attach as a file. Prefer English. Follow ASD-STE100 Issue 9 in at least 80% of sentences in each text. Russian is permitted under the same rules. Preserve verbatim user text. Use clean Markdown, and write an object reference as [Title](kind:id), never a bare ID. Do not put link lists or progress notes in this text: use links and the note op. Read joppa_help with section writing.";

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
            tool("joppa_list", "List accessible product workspaces in pages of 25: items {id, title, hidden}, total and next_cursor; send next_cursor back as cursor. hidden is true only for a workspace that update_workspace hid from the web lists; MCP reads and writes it as usual.", json!({"cursor":{"type":"string"}}), json!([])),
            tool("joppa_search", "Find objects of any type with full-text and vector search. Each result gives address, title, revision, score and excerpt; a page gives total and next_cursor. Results are references, not acceptance evidence: read the object before a change. A list reply larger than 256 KB is refused.", schemas::search(), json!(["workspace","query"])),
            tool("joppa_tree", "Read the versioned workspace tree schema. Type index sets presentation order; smaller values appear first. Type title_size sets title size in pixels (12 to 48). Type badge sets the compact label. Type title_max_length is the maximum title length on write and the visible title length (default 128 characters). Type field_max_length is the maximum length of each new or changed data value on write (1 to 262144 characters, default 1024). Over 1 kb: prefer attach as a file. The reply gives this advice as field_hint. A workspace member changes it with set_tree_schema: types replaces the whole map, put_types adds or replaces single types, remove_types deletes single types. Send schema.revision with set_tree_schema. A type gives inbox, root and launch_blocker only when they are true. Each type names its status rule in status: own, children, links or none. A node of a type with role test and rule own names one test in its title. status_required=true refuses a type without status; with false its nodes are orange (unset). filters names up to 16 data fields, such as tags or priority, that the board offers as filter drop-downs; a change without filters keeps the list. test_results holds every test rule: statuses maps each status name to pass, fail, skip or todo; fields lists the result fields {name, title?, type: text|number|enum|ref, target?, required?, values?, max_length?}; name, max_tests, max_results, same_commit, reset_on_change, green_if_all, red_if_any and failure_field. A set_tree_schema with test_results sends every rule. The browser page is read-only.", json!({"workspace":{"type":"string"},"revision":{"type":"integer","minimum":0}}), json!(["workspace"])),
            tool("joppa_objects", "Read objects. Without address, a list page gives at most 25 items {address, title, status tone} with total and next_cursor; fields [\"children\"] adds the number of present children, and fields [\"links\"] adds the stored links {target, relation, blocking, revision when pinned}. With address, the reply is one object: revision, parent, data, status, process, comments, links and link_groups; 2 to 25 addresses give {items}. A list reply larger than 256 KB is refused; one object comes in full. No reply repeats the request, and an omitted key is empty.", schemas::objects(), json!([])),
            tool("joppa_timeline", "Keys with null, \"\", [] or {} are omitted outside document, data, command and remote_links. Read the timeline of one workspace: one entry for each recorded change, in journal order. Each entry has position, at, actor, address, title, revision, change, op and reason. A comment entry (op comment, note or withdraw_note) also has comment_id. change is save, move, archive, restore, comment, file, tests, readiness, priority, task, results, planning, member, schema, workspace or other. revision is the object revision at that change. A record of test results has the change results and the address record/ID. Supply after to read only the entries after that position. A page gives at most 25 entries with total and next_cursor; follow next_cursor until it is missing. order=newest reads the newest entries first.", json!({"workspace":{"type":"string"},"after":{"type":"integer","minimum":0,"default":0},"limit":{"type":"integer","minimum":1,"maximum":25,"default":25},"cursor":{"type":"string"},"order":{"type":"string","enum":["oldest","newest"],"default":"oldest"}}), json!(["workspace"])),
            tool("joppa_record_results", "Record the test outcomes of one commit in one call. Supply commit (a lowercase git sha) and results: [{test, status, ...fields}]. The block test_results of joppa_tree gives the status names, the fields of a result and max_results. A larger run uses several calls. Joppa sets the actor from the session and the time from its clock; a call that supplies actor or at is refused. The latest record of a test decides it. An omitted test gets no result. The rules green_if_all and red_if_any of the block decide each test node whose title is the test name. The reply gives unbound: the tests of the record whose name is the title of no present test node in the workspace. unbound does not check under which criterion a test node stands. A full-run record must give unbound: [] before a merge: save a test node for each unbound test, or delete the test.", json!({"workspace":{"type":"string"},"commit":{"type":"string"},"results":{"type":"array","minItems":1,"items":{"type":"object","properties":{"test":{"type":"string"},"status":{"type":"string","description":"a name from the tree schema test_results.statuses"}},"required":["test","status"],"additionalProperties":true}}}), json!(["workspace","commit","results"])),
            tool("joppa_history", "Keys with null, \"\", [] or {} are omitted outside document, data, command and remote_links. Read the History of one workspace: journal entries, each open until a member marks it processed. It returns the oldest open entries, at most 25 a page, with total and next_cursor; next_item names the oldest open entry. An entry gives no object status; read it with joppa_objects. Follow next_cursor for the next page. all=true also returns processed entries with their actor, at, reason and nodes.", json!({"workspace":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":25,"default":25},"cursor":{"type":"string"},"all":{"type":"boolean"}}), json!(["workspace"])),
            tool("joppa_history_mark", "Mark one open History entry processed. Supply position, a short reason and at most 15 nodes that the work created or changed, each as an address or a public item ID; the mark stores the address. Joppa sets the actor and the time; a call that supplies actor or at is refused. A mark adds no journal entry, and an entry is marked once.", json!({"workspace":{"type":"string"},"position":{"type":"integer","minimum":1},"reason":{"type":"string"},"nodes":{"type":"array","maxItems":15,"items":{"type":"string"}}}), json!(["workspace","position","reason"])),
            tool("joppa_command", "Append an operation. Each new or changed value in object data holds at most the field_max_length of its type (default 1024 characters); split a longer text into child objects or attach a file with joppa_attach. Over 1 kb: prefer attach as a file. Link objects only with the links array of save_object or schema reference fields, never with lists in object text. Add findings, progress and supplements only as comments: {\"op\":\"note\",\"target\":\"kind:id\",\"id\":\"<unique>\",\"text\":\"...\"}; never rewrite or extend object text for them. A note id has 1 to 120 ASCII letters, digits, \"-\", \"_\" or \".\". Read joppa_help for operation schemas and ASD-STE100 writing rules. Prefer English for new technical text. Actor comes from the authenticated credential. Reuse request_id only for identical retries. A test is a node: save_object {kind: \"test\", parent: \"ac:x\", data: {title: \"<test name>\", owner}} (or parent: \"tac:x\"); one node names one test. Every test in the repository must have a present test node: no criterion, no test. Save a node for a new or renamed test, and archive the node of a deleted test. The reply gives position and the saved object {address, revision}; command then gives only its op, and replayed shows only when true. set_tree_schema gives schema_revision instead of the sent types.",json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"command":authoring_command_schema()}), json!(["workspace","request_id","command"])),
            tool("joppa_batch", "Append up to 100 operations atomically. Each new or changed value in object data holds at most the field_max_length of its type (default 1024 characters); split a longer text into child objects or attach a file with joppa_attach. Over 1 kb: prefer attach as a file. Link objects only with the links array of save_object or schema reference fields, never with lists in object text. Add findings, progress and supplements only as comments: {\"op\":\"note\",\"target\":\"kind:id\",\"id\":\"<unique>\",\"text\":\"...\"}; never rewrite or extend object text for them. A note id has 1 to 120 ASCII letters, digits, \"-\", \"_\" or \".\". Optional ref labels let later commands refer to generated IDs with {\"$ref\":\"label/id\"}, and to a saved object with {\"$ref\":\"label/address\"}, for example as the parent. Optional defaults gives save_object fields, for example reason and data.owner: each save_object of the batch takes each default field that it omits, data merges by key, and a field of the command wins. Read joppa_help for schemas and ASD-STE100 writing rules. Prefer English for new technical text. Retry identical input with the same request_id.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"defaults":{"type":"object","description":"save_object fields for each save_object of the batch that omits them, for example {\"reason\":\"...\",\"data\":{\"owner\":\"...\"}}. No op."},"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","properties":{"ref":{"type":"string"},"command":authoring_command_schema()},"required":["command"],"additionalProperties":false}}}), json!(["workspace","request_id","operations"])),
            tool("joppa_help", "Get operation shapes and writing rules. Read before authoring commands. Without section or op, the reply gives a short table of contents: sections, ops and hint. Supply section (operations, writing, objects, search, tree, examples) to read one section, or op to read the shape of one operation. The sections tree and examples need workspace: tree gives the types with their questions. The title and body of an object answer the question of its type. Over 1 kb: prefer attach as a file. The reply gives this advice as field_hint.", json!({"workspace":{"type":"string"},"section":{"type":"string","enum":["operations","writing","objects","search","tree","examples"],"description":"One section of the help."},"op":{"type":"string","description":"One operation name: the reply gives its required fields and callers."}}), json!([]))
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
    properties["alias"] = json!({"type":"string","description":"Optional: the name of the agent that writes under this account, such as p1. History, the timeline, revisions, comments and files show it next to the actor. It changes no access. The header X-Joppa-Alias sets it for a session; this argument wins."});
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
fn authoring_command_schema() -> Value {
    let text = || json!({"description":AUTHORING_TEXT});
    json!({
        "type":"object",
        "additionalProperties":true,
        "description":"Operation fields depend on op. Read joppa_help with section operations. Apply joppa_help section writing to new technical text. Each field that names an object of the workspace takes its address or its public item ID, such as joppa-R-DP6KXM62: target, parent, duplicate_of, links[].target, the reference fields of data and the id of save_object. The journal stores the kind:id address.",
        "properties":{
            "op":{"type":"string","enum":joppa::commands::OPERATIONS.iter().map(|o| o.name).collect::<Vec<_>>(),"description":"Operation name. Read joppa_help with this op for its required fields and callers."},
            "revision":{"description":"For save_object, archive_object, restore_object and set_tree_schema, send the revision you read. Zero creates a new object or changes the default schema. The server creates the next revision."},
            "data":{"type":"object","description":"Document fields. data.owner: the member ID of a human owner, required on every save; an AC without it takes the owner of its REQ. data.title: one concise statement (max 128 chars). data.body: the text of every object type; data.description, data.contract and data.decision_required are refused. Each other text value: concise text (max field_max_length of the type, default 1024 chars). Over 1 kb: prefer attach as a file. Put links in links and comments in note, never in these texts."},
            "title":{"description":"One concise statement (max 128 chars). Prefer English. Follow ASD-STE100 Issue 9."}, "body":text(), "text":text(),
            "reason":text(), "value":text(),
            "hidden":{"type":"boolean","description":"For update_workspace: true hides the workspace from the web lists, false shows it again."},
            "ready":{"type":"boolean","description":"For assess: true when the node is ready for work, false when it waits; reason says for what."},
            "expected_position":{"type":"integer","minimum":0,"description":"For assess, optional: the process.assessment.position that you read, 0 for no mark. A newer mark refuses the write: stale assessment; read the object again."}
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
        "joppa_timeline" => pages::timeline(pool, actor, a).await,
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
        "joppa_tests" => anyhow::bail!(
            "joppa_tests was removed; use joppa_objects with kind test, for example {{\"parent\":\"ac:x\",\"kind\":\"test\"}}"
        ),
        _ => anyhow::bail!("unknown tool"),
    };
    let strip = ["joppa_timeline", "joppa_history", "joppa_search"];
    if strip.contains(&name) {
        let all_workspaces = a["all_workspaces"] == true;
        return reply
            .map(|reply| match name {
                "joppa_timeline" => shapes::timeline(reply),
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
