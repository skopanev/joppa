use deadpool_postgres::Pool;
use joppa::{mcp_shapes as shapes, store};
use serde_json::{Value, json};

#[path = "protocol_files.rs"]
mod files;
#[path = "protocol_pages.rs"]
mod pages;
#[path = "protocol_reads.rs"]
mod reads;
#[path = "protocol_tests.rs"]
mod test_reads;

const WRITING_RULES: &str = include_str!("../docs/writing.md");
const AUTHORING_TEXT: &str = "Concise text (max field_max_length of the type, default 1024 chars). Over 1 kb: prefer attach as a file. Prefer English. Follow ASD-STE100 Issue 9 in at least 80% of sentences in each text. Russian is permitted under the same rules. Preserve verbatim user text. Use clean Markdown, and write an object reference as [Title](kind:id), never a bare ID. Do not put link lists or progress notes in this text: use links and the note op. Read joppa_help.writing.";

pub async fn handle(
    pool: &Pool,
    actor: &str,
    access: store::Access,
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
        "tools/list" => Ok(json!({"tools": json!([
            tool("joppa_list", "List accessible product workspaces in pages of 25: items {id, title}, total and next_cursor; send next_cursor back as cursor.", json!({"cursor":{"type":"string"}}), json!([])),
            tool("joppa_search", "Keys with null, \"\", [] or {} are omitted outside document, data, command and remote_links. Find objects of any configured type. Each search combines full-text and vector search. Each result gives its address, title, revision, score and excerpt, the text of the object. By default it reads the current revision of live objects in one workspace. all_versions=true also reads each past revision and archived objects; a result gives historical: true or archived: true when they apply. all_workspaces=true reads each workspace that you can read; only then each result gives its workspace. A result of a criterion gives req, its requirement. A page gives limit results (default 10, at most 25) with total and next_cursor; send next_cursor back as cursor. Results are references, not acceptance evidence. Read the matching item before changes.", json!({"workspace":{"type":"string"},"query":{"type":"string","minLength":1,"maxLength":2000},"kind":{"type":"string"},"limit":{"type":"integer","minimum":1,"default":10},"cursor":{"type":"string"},"all_versions":{"type":"boolean","default":false},"all_workspaces":{"type":"boolean","default":false}}), json!(["workspace","query"])),
            tool("joppa_tests", "At most 100 criteria in one call. Instead of ac, send parent (direct children, or * for all) and kind (default ac) to read every criterion in pages of 100 with next_cursor and total. A missing tests key means no tests. Read the test status of one criterion or of several, separated by commas: green when every listed test passes, red when one fails or is missing, grey otherwise. Each test shows its latest status, commit and failure message. A Task has no tests of its own: pass task to read done and commit. Only complete_task sets done and commit: complete_task {id, revision, evidence, commit}, where commit is the git sha where the Task is ready. The list is the field tests of the criterion document: save_object with data.tests saves it, and the short command set_ac_tests {target, tests: [\"file > describe > name\"], revision, reason} saves only that field. revision is optional: send the tests_revision you read, the document revision of the criterion, also when its list is empty. A different revision fails with \"stale test list revision; read the tests again\". Without ac and task, the reply is the overview of green and red criteria in pages of 25 with total and next_cursor; send next_cursor back as cursor. summary=true replaces tests[] of each criterion with counts per test status.", json!({"workspace":{"type":"string"},"parent":{"type":"string"},"kind":{"type":"string"},"cursor":{"type":"string"},"ac":{"type":"string"},"task":{"type":"string"},"summary":{"type":"boolean"}}), json!(["workspace"])),
            tool("joppa_tree", "Read the versioned workspace tree schema. Type index sets presentation order; smaller values appear first. Type title_size sets title size in pixels (12 to 48). Type badge sets the compact label. Type title_max_length is the maximum title length on write and the visible title length (default 128 characters). Type field_max_length is the maximum length of each new or changed data value on write (1 to 262144 characters, default 1024). Over 1 kb: prefer attach as a file. The reply gives this advice as field_hint. A workspace member changes it with set_tree_schema: types replaces the whole map, put_types adds or replaces single types, remove_types deletes single types. Send schema.revision with set_tree_schema. A type gives inbox, root and launch_blocker only when they are true. Each type names its status rule in status: tests, children, links or none. status_required=true refuses a type without status; with false its nodes are orange (unset). A node with status tests keeps its test names in the document field tests. test_results holds every test rule: statuses maps each status name to pass, fail, skip or todo; fields lists the result fields {name, title?, type: text|number|enum|ref, target?, required?, values?, max_length?, board?, applies?}; name, max_tests, max_results, same_commit, reset_on_change, green_if_all, red_if_any and failure_field. A set_tree_schema with test_results sends every rule. board=true on an enum or ref field gives each value its own criterion status and a board filter; applies names the criterion reference that lists the values that apply, or the explicit value all. The browser page is read-only.", json!({"workspace":{"type":"string"},"revision":{"type":"integer","minimum":0}}), json!(["workspace"])),
            tool("joppa_objects", "Read objects. A list (no address) gives at most 25 items {address, title, status tone, children} with total and next_cursor; send next_cursor back as cursor. No read gives the workspace or a snapshot position, or repeats the address that the request sent. parent is a node address, * for every object, or absent for the roots; kind filters by type. fields names the keys of each item as dotted paths, each named path comes back, for example [\"title\",\"data.description\"]; data and data.<key> give the full document data. An Inbox type (inbox=true in joppa_tree) lists unprocessed inputs first; note_status=unprocessed gives only open inputs. related_to reads links in both directions. work_filter=unstarted lists the criteria that are ready, not verified as passing, and have no active Task; a blocked Task does not count as active. Task binding follows the nearest criterion through the tree parents and stops at a specification. work_summary counts active, done and retired Tasks; check work_summary.done before creating new work. With address you get the current object: revision (send it with a write), parent, data, status, process, comments in pages of 25 (send comments_cursor), all outgoing links as one list, incoming link counts in link_groups, remote_links and child_workspace. A link gives target and title, and relation when it is not related; a pin adds revision and current_revision, and a blocking link adds blocking: true. save_object takes links back as they are; a link without relation is related. links_of=<address> reads its links in pages of 25 with total; an item gives present: false, blocking: true and a relation other than related only when they apply; direction, relation and kind are optional filters, and an item names each one that the request did not send. revision=N reads one revision: revision, parent, data, links and current_revision; history=true gives the revision headers {revision, at, actor, reason} as items, oldest first, in pages of 25 with total and next_cursor. address takes 2 to 10 addresses separated by commas: the reply is {items}, each item is one object with its address; history, revision and comments_cursor read one address. With address, fields gives the address only for more addresses or an alias, the revision and each named path. A reply larger than 256 KB is refused; narrow it with fields, fewer addresses or a smaller limit. An omitted key is empty: no value, [] or 0.", json!({"workspace":{"type":"string"},"comments_cursor":{"type":"string"},"fields":{"type":"array","items":{"type":"string"}},"address":{"type":"string"},"parent":{"type":"string"},"kind":{"type":"string"},"revision":{"type":"integer","minimum":1},"history":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"string"},"note_status":{"type":"string","enum":["unprocessed","processed"]},"related_to":{"type":"string"},"work_filter":{"type":"string","enum":["unstarted"]},"links_of":{"type":"string"},"direction":{"type":"string","enum":["outgoing","incoming"]},"relation":{"type":"string"}}), json!(["workspace"])),
            tool("joppa_tasks", "Keys with null, \"\", [] or {} are omitted outside document, data, command and remote_links. List current Tasks without reading every REQ. available shows tasks this actor can claim, mine shows claimed unfinished tasks, waiting shows unassigned tasks whose configured parent gate, dependencies, or contract are not ready, all shows every current Task. A blocked Task is not claimable; its waiting_reason is \"Task is blocked for planning\". A task gives claimable and review_required only when they are true. Optional filters and ordered priorities use the same selection as claim_next_task. A page gives at most 25 tasks as items with total and next_cursor; send next_cursor back as cursor. A cursor expires when the workspace changes.", json!({"workspace":{"type":"string"},"view":{"type":"string","enum":["available","mine","waiting","all"],"default":"available"},"limit":{"type":"integer","minimum":1,"default":25},"cursor":{"type":"string"},"project":{"type":"string"},"purpose":{"type":"string","minLength":1},"priorities":{"type":"array","items":{"type":"string"}}}), json!(["workspace"])),
            tool("joppa_timeline", "Keys with null, \"\", [] or {} are omitted outside document, data, command and remote_links. Read the timeline of one workspace: one entry for each recorded change, in journal order. Each entry has position, at, actor, actor_type, address, title, revision, change, op and reason. change is save, move, archive, restore, comment, file, tests, readiness, priority, task, results, planning, member, schema, workspace or other. revision is the object revision at that change. A record of test results has the change results and the address record/ID. Supply after to read only the entries after that position. A page gives at most 25 entries with total and next_cursor; follow next_cursor until it is missing. order=newest reads the newest entries first.", json!({"workspace":{"type":"string"},"after":{"type":"integer","minimum":0,"default":0},"limit":{"type":"integer","minimum":1,"default":25},"cursor":{"type":"string"},"order":{"type":"string","enum":["oldest","newest"],"default":"oldest"}}), json!(["workspace"])),
            tool("joppa_record_results", "Record the test outcomes of one commit in one call. Supply commit (a lowercase git sha) and results: [{test, status, ...fields}]. The block test_results of joppa_tree gives the status names, the fields of a result and max_results. A larger run uses several calls. Joppa sets the actor from the session and the time from its clock; a call that supplies actor or at is refused. The latest record of a test decides it. An omitted test gets no result. The rules green_if_all and red_if_any of the block decide the criterion.", json!({"workspace":{"type":"string"},"commit":{"type":"string"},"results":{"type":"array","minItems":1,"items":{"type":"object","properties":{"test":{"type":"string"},"status":{"type":"string","description":"a name from the tree schema test_results.statuses"}},"required":["test","status"],"additionalProperties":true}}}), json!(["workspace","commit","results"])),
            tool("joppa_history", "Keys with null, \"\", [] or {} are omitted outside document, data, command and remote_links. Read the History of one workspace: journal entries, each open until a member marks it processed. It returns the oldest open entries, at most 25 a page, with total and next_cursor; next_item names the oldest open entry. An entry gives no object status; read it with joppa_objects. Follow next_cursor for the next page. all=true also returns processed entries with their actor, at, reason and nodes.", json!({"workspace":{"type":"string"},"limit":{"type":"integer","minimum":1,"default":25},"cursor":{"type":"string"},"all":{"type":"boolean"}}), json!(["workspace"])),
            tool("joppa_history_mark", "Mark one open History entry processed. Supply position, a short reason and at most 15 node addresses that the work created or changed. Joppa sets the actor and the time; a call that supplies actor or at is refused. A mark adds no journal entry, and an entry is marked once.", json!({"workspace":{"type":"string"},"position":{"type":"integer","minimum":1},"reason":{"type":"string"},"nodes":{"type":"array","maxItems":15,"items":{"type":"string"}}}), json!(["workspace","position","reason"])),
            tool("joppa_command", "Append an operation. Each new or changed value in object data holds at most the field_max_length of its type (default 1024 characters); split a longer text into child objects or attach a file with joppa_attach. Over 1 kb: prefer attach as a file. Link objects only with the links array of save_object or schema reference fields, never with lists in object text. Add findings, progress and supplements only as comments: {\"op\":\"note\",\"target\":\"kind:id\",\"id\":\"<unique>\",\"text\":\"...\"}; never rewrite or extend object text for them. Read joppa_help for operation schemas and ASD-STE100 writing rules. Prefer English for new technical text. Actor comes from the authenticated credential. Reuse request_id only for identical retries. The reply gives position and the saved object {address, revision}; command then gives only its op, and replayed shows only when true. set_tree_schema gives schema_revision instead of the sent types. The holder or the workspace owner stops failed work with block_task {id, revision, reason, evidence}: it clears the holder, keeps status open and records process.blocked; claims then refuse the Task. When the failure was outside the Task, the planning holder of the parent criterion or the workspace owner clears the block with unblock_task {id, revision, reason, evidence}: it sets process.blocked to null and keeps the contract and process.origin_revision. finish_planning {ac, reason} compares each blocked, unretired child Task with its document at the block: title, contract, projects and blocking links. If one is unchanged, finish fails with \"blocked Task needs a new contract or retirement\". A successful finish clears process.blocked of the changed Tasks. retire_task {id, revision, reason} needs the Task owner or the workspace owner. The planning holder of the parent criterion can also retire a blocked Task.",json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"command":authoring_command_schema()}), json!(["workspace","request_id","command"])),
            tool("joppa_batch", "Append up to 100 operations atomically. Each new or changed value in object data holds at most the field_max_length of its type (default 1024 characters); split a longer text into child objects or attach a file with joppa_attach. Over 1 kb: prefer attach as a file. Link objects only with the links array of save_object or schema reference fields, never with lists in object text. Add findings, progress and supplements only as comments: {\"op\":\"note\",\"target\":\"kind:id\",\"id\":\"<unique>\",\"text\":\"...\"}; never rewrite or extend object text for them. Optional ref labels let later commands refer to generated IDs with {\"$ref\":\"label/id\"}, and to a saved object with {\"$ref\":\"label/address\"}, for example as the parent. Read joppa_help for schemas and ASD-STE100 writing rules. Prefer English for new technical text. Retry identical input with the same request_id.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","properties":{"ref":{"type":"string"},"command":authoring_command_schema()},"required":["command"],"additionalProperties":false}}}), json!(["workspace","request_id","operations"])),
            tool("joppa_help", "Get operation shapes and writing rules. Read before authoring commands. Over 1 kb: prefer attach as a file. The reply gives this advice as field_hint. Supply workspace to read schema questions. Check the title and description, then resend save_object with question_checked equal to the question.", json!({"workspace":{"type":"string"}}), json!([]))
        ]).as_array().into_iter().flatten().chain(files::tools().iter())
            // A read-only grant sees only the tools that it can call.
            .filter(|t| access != store::Access::Read || t["name"].as_str().is_some_and(|n| READ_TOOLS.contains(&n)))
            .cloned().collect::<Vec<_>>()})),
        "tools/call" => call(pool, actor, access, msg, embedder)
            .await
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
pub(crate) fn tool(name: &str, description: &str, properties: Value, required: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
fn authoring_command_schema() -> Value {
    let text = || json!({"description":AUTHORING_TEXT});
    json!({
        "type":"object",
        "additionalProperties":true,
        "description":"Operation fields depend on op. Read joppa_help.operations. Apply joppa_help.writing to new technical text.",
        "properties":{
            "op":{"type":"string","enum":joppa::commands::OPERATIONS.iter().map(|o| o.name).collect::<Vec<_>>(),"description":"Operation name. Read joppa_help.operations for its required fields and callers."},
            "question_checked":{"type":"string","description":"For save_object, use the exact configured type question after checking that the title and description answer it. Read joppa_help with workspace first."},
            "revision":{"description":"For save_object, archive_object, restore_object and set_tree_schema, send the revision you read. Zero creates a new object or changes the default schema. The server creates the next revision. For set_ac_tests, revision is optional: send the tests_revision you read, the document revision of the criterion, also when its list is empty."},
            "data":{"type":"object","description":"Document fields. data.title: one concise statement (max 128 chars). Each other text value: concise text (max field_max_length of the type, default 1024 chars). Over 1 kb: prefer attach as a file. Put links in links and comments in note, never in these texts."},
            "title":{"description":"One concise statement (max 128 chars). Prefer English. Follow ASD-STE100 Issue 9."}, "body":text(), "text":text(), "description":text(),
            "reason":text(), "contract":text(), "value":text(),
            "assessment":{"properties":{"reason":text()}},
            "evidence":{"type":"string","description":"For complete_task, use technical text or the required decision comment identifier. For block_task, give the failure facts, such as the failed test and commit. For unblock_task, give the facts that show the failure was outside the Task, such as the run. Preserve exact facts, logs, code, identifiers, and quotations."}
        }
    })
}
/// The only tools a read grant may call. Any other tool, known or not, is refused.
const READ_TOOLS: &[&str] = &[
    "joppa_list",
    "joppa_search",
    "joppa_tests",
    "joppa_tree",
    "joppa_objects",
    "joppa_tasks",
    "joppa_timeline",
    "joppa_history",
    "joppa_help",
    "joppa_attachments",
];
async fn call(
    pool: &Pool,
    actor: &str,
    access: store::Access,
    msg: &Value,
    embedder: Option<&joppa::embeddings::Embedder>,
) -> anyhow::Result<Value> {
    let a = &msg["params"]["arguments"];
    let field = |k: &str| a[k].as_str().ok_or_else(|| anyhow::anyhow!("missing {k}"));
    let name = msg["params"]["name"].as_str().unwrap_or("");
    // Refused before any store call.
    if access == store::Access::Read && !READ_TOOLS.contains(&name) {
        anyhow::bail!(store::READ_ONLY_REFUSAL);
    }
    // requirement:read-contract: the compact read shape.
    if matches!(name, "joppa_objects" | "joppa_tests") {
        return reads::compact(pool, actor, name, a).await;
    }
    let reply = match name {
        "joppa_list" => pages::list(pool, actor, a).await,
        "joppa_search" => pages::search(pool, actor, a, embedder).await,
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
        "joppa_tasks" => pages::task_queue(pool, actor, a).await,
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
        "joppa_help" => {
            let operations: serde_json::Map<String, Value> = joppa::commands::OPERATIONS
                .iter()
                .map(|o| {
                    (
                        o.name.to_owned(),
                        json!({"required":o.required,"callers":o.callers}),
                    )
                })
                .collect();
            let mut help = json!({"operations":operations,"search":include_str!("../docs/search.md"),"writing":WRITING_RULES,"objects":include_str!("../docs/objects.md")});
            if a.get("workspace").is_some() {
                help["tree"] = store::tree_schema(pool, field("workspace")?, actor, None).await?;
                let types = serde_json::from_value(help["tree"]["schema"]["types"].clone())?;
                help["examples"] = joppa::save_examples::save_examples(&types);
            }
            Ok(help)
        }
        _ => anyhow::bail!("unknown tool"),
    };
    let strip = [
        "joppa_tasks",
        "joppa_timeline",
        "joppa_history",
        "joppa_search",
    ];
    if strip.contains(&name) {
        let all_workspaces = a["all_workspaces"] == true;
        return reply
            .map(|reply| match name {
                "joppa_tasks" => shapes::tasks(reply),
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
