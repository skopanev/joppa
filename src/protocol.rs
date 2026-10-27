use deadpool_postgres::Pool;
use joppa::store;
use serde_json::{Value, json};

const WRITING_RULES: &str = include_str!("../docs/writing.md");
const AUTHORING_TEXT: &str = "Prefer English. Follow ASD-STE100 Issue 9 in at least 80% of sentences in each text. Russian is permitted under the same rules. Preserve verbatim user text. Read joppa_help.writing.";

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
            tool("joppa_list", "List accessible product workspaces", json!({}), json!([])),
            tool("joppa_search", "Find current objects of any configured type inside one authorized workspace. Combines full-text and semantic search when embeddings are available; reports coverage. Results are references, not acceptance evidence. Read the matching item before changes.", json!({"workspace":{"type":"string"},"query":{"type":"string","minLength":1,"maxLength":2000},"kind":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":50,"default":10}}), json!(["workspace","query"])),
            tool("joppa_read", "Read a schema-defined root index, one copied object, or its configured verification context. Supply workspace OR item_id. Every requirement reports revision_count. history=true returns counts only, never events or revision bodies. To read an older revision, explicitly supply revision=N; the response contains exactly one revision and its discussion/evidence. Copied requirement and AC IDs obey the same rules.", json!({"workspace":{"type":"string"},"item_id":{"type":"string"},"req":{"type":"string","description":"One requirement: current revision by default, including its ACs, tasks, runs, issues and revision comments"},"revision":{"type":"integer","minimum":1,"description":"Read exactly this revision of the selected requirement; omit for current. Works with req or a requirement/AC item_id."},"history":{"type":"boolean","default":false,"description":"Discover revision counts without revision bodies or events. Then request one revision by number."},"target":{"type":"string","description":"One object's notes: description, comments and attachments. Address as kind:id, e.g. requirement:r-xxx or ac:r-xxx/2. Cannot be combined with revision or history."}}), json!([])),
            tool("joppa_tests", "Read the test status of one criterion or of several, separated by commas: green when every listed test passes, red when one fails or is missing, grey otherwise. Each test shows its latest status, commit and failure message. A Task has no tests of its own: pass task to read done and commit. Only complete_task sets done and commit: complete_task {id, revision, evidence, commit}, where commit is the git sha where the Task is ready. The list is the field tests of the criterion document: save_object with data.tests saves it, and the short command set_ac_tests {target, tests: [\"file > describe > name\"], revision, reason} saves only that field. revision is optional: send the tests_revision you read, the document revision of the criterion, also when its list is empty. A different revision fails with \"stale test list revision; read the tests again\". Without ac and task, the reply is the overview of green and red criteria. summary=true replaces tests[] of each criterion with counts per test status.", json!({"workspace":{"type":"string"},"ac":{"type":"string"},"task":{"type":"string"},"summary":{"type":"boolean"}}), json!(["workspace"])),
            tool("joppa_tree", "Read the versioned workspace tree schema. Type index sets presentation order; smaller values appear first. Type title_size sets title size in pixels (12 to 48). Type badge sets the compact label. Type title_preview_length is the maximum title length on write and the visible title length (default 64 characters). A workspace member changes it with set_tree_schema: types replaces the whole map, put_types adds or replaces single types, remove_types deletes single types. Each type names its status rule in status: tests, children, links or none. status_required=true refuses a type without status; with false its nodes are orange (unset). A node with status tests keeps its test names in the document field tests. test_results holds every test rule: statuses maps each status name to pass, fail, skip or todo; fields lists the result fields {name, title?, type: text|number|enum|ref, target?, required?, values?, max_length?, board?, applies?}; name, max_tests, max_results, same_commit, reset_on_change, green_if_all, red_if_any and failure_field. A set_tree_schema with test_results sends every rule. board=true on an enum or ref field gives each value its own criterion status and a board filter; applies names the criterion reference that lists the values that apply, or the explicit value all. The browser page is read-only.", json!({"workspace":{"type":"string"},"revision":{"type":"integer","minimum":0}}), json!(["workspace"])),
            tool("joppa_objects", "Read one common object or list its children. Every object has revisions, links, comments and attachments. Address uses kind:id. No address means root objects. parent=* lists all objects. Lists are paginated. history=true returns counts. revision=N reads exactly one document revision. For the Inbox, use kind of a schema type with inbox=true and parent=*. Unprocessed inputs come first, then processed inputs; each group uses address order. Items expose status and note_status. Set note_status=unprocessed for only open inputs. Without parent=* the list holds only root notes and misses notes under other objects. related_to reads links in both directions. work_filter=unstarted lists all configured criteria that are ready, not verified as passing, and have no active Tasks. A blocked Task does not count as active. Task binding follows the nearest criterion through configured tree parents and stops at a specification. Check work_summary.done before creating new work. work_summary counts active, done and retired Tasks. Process evidence remains available through joppa_read. One object read also gives remote_links, the links to other workspaces with their current status, and child_workspace for a node that stands for another workspace. fields keeps only the named keys of each list item, for example [\"title\",\"signal\"]; the address always stays.", json!({"workspace":{"type":"string"},"fields":{"type":"array","items":{"type":"string"}},"address":{"type":"string"},"parent":{"type":"string"},"kind":{"type":"string"},"revision":{"type":"integer","minimum":1},"history":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"string"},"note_status":{"type":"string","enum":["unprocessed","processed"]},"related_to":{"type":"string"},"work_filter":{"type":"string","enum":["unstarted"]}}), json!(["workspace"])),
            tool("joppa_tasks", "List current Tasks without reading every REQ. available shows tasks this actor can claim, mine shows claimed unfinished tasks, waiting shows unassigned tasks whose configured parent gate, dependencies, or contract are not ready, all shows every current Task. A blocked Task is not claimable; its waiting_reason is \"Task is blocked for planning\". Optional filters and ordered priorities use the same selection as claim_next_task. Results are paginated; a cursor expires when the workspace changes.", json!({"workspace":{"type":"string"},"view":{"type":"string","enum":["available","mine","waiting","all"],"default":"available"},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50},"cursor":{"type":"string"},"project":{"type":"string"},"purpose":{"type":"string","minLength":1},"priorities":{"type":"array","items":{"type":"string"}}}), json!(["workspace"])),
            tool("joppa_timeline", "Read the timeline of one workspace: one entry for each recorded change, in journal order. Each entry has position, at, actor, actor_type, address, title, revision, change, op and reason. change is save, move, archive, restore, comment, file, tests, readiness, priority, task, results, planning, member, schema, workspace or other. revision is the object revision at that change. A record of test results has the change results and the address record/ID. Supply after to read only the entries after that position. Follow next_cursor until it is null. order=newest reads the newest entries first.", json!({"workspace":{"type":"string"},"after":{"type":"integer","minimum":0,"default":0},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50},"cursor":{"type":"string"},"order":{"type":"string","enum":["oldest","newest"],"default":"oldest"}}), json!(["workspace"])),
            tool("joppa_record_results", "Record the test outcomes of one commit in one call. Supply commit (a lowercase git sha) and results: [{test, status, ...fields}]. The block test_results of joppa_tree gives the status names, the fields of a result and max_results. A larger run uses several calls. Joppa sets the actor from the session and the time from its clock; a call that supplies actor or at is refused. The latest record of a test decides it. An omitted test gets no result. The rules green_if_all and red_if_any of the block decide the criterion.", json!({"workspace":{"type":"string"},"commit":{"type":"string"},"results":{"type":"array","minItems":1,"items":{"type":"object","properties":{"test":{"type":"string"},"status":{"type":"string","description":"a name from the tree schema test_results.statuses"}},"required":["test","status"],"additionalProperties":true}}}), json!(["workspace","commit","results"])),
            tool("joppa_history", "Read the History of one workspace: journal entries, each open until a member marks it processed. By default it returns the oldest open entry; next_item names it. limit takes 1 to 25. Follow next_cursor for the next page. all=true also returns processed entries with their actor, at, reason and nodes.", json!({"workspace":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":25},"cursor":{"type":"string"},"all":{"type":"boolean"}}), json!(["workspace"])),
            tool("joppa_history_mark", "Mark one open History entry processed. Supply position, a short reason and at most 15 node addresses that the work created or changed. Joppa sets the actor and the time; a call that supplies actor or at is refused. A mark adds no journal entry, and an entry is marked once.", json!({"workspace":{"type":"string"},"position":{"type":"integer","minimum":1},"reason":{"type":"string"},"nodes":{"type":"array","maxItems":15,"items":{"type":"string"}}}), json!(["workspace","position","reason"])),
            tool("joppa_command", "Append an operation. Read joppa_help for operation schemas and ASD-STE100 writing rules. Prefer English for new technical text. Actor comes from the authenticated credential. Reuse request_id only for identical retries. The holder or the workspace owner stops failed work with block_task {id, revision, reason, evidence}: it clears the holder, keeps status open and records process.blocked; claims then refuse the Task. When the failure was outside the Task, the planning holder of the parent criterion or the workspace owner clears the block with unblock_task {id, revision, reason, evidence}: it sets process.blocked to null and keeps the contract and process.origin_revision. finish_planning {ac, reason} compares each blocked, unretired child Task with its document at the block: title, contract, projects and blocking links. If one is unchanged, finish fails with \"blocked Task needs a new contract or retirement\". A successful finish clears process.blocked of the changed Tasks. retire_task {id, revision, reason} needs the Task owner or the workspace owner. The planning holder of the parent criterion can also retire a blocked Task.",json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"command":authoring_command_schema()}), json!(["workspace","request_id","command"])),
            tool("joppa_batch", "Append up to 100 operations atomically. Optional ref labels let later commands refer to generated IDs with {\"$ref\":\"label/id\"}. Read joppa_help for schemas and ASD-STE100 writing rules. Prefer English for new technical text. Retry identical input with the same request_id.", json!({"workspace":{"type":"string"},"request_id":{"type":"string"},"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","properties":{"ref":{"type":"string"},"command":authoring_command_schema()},"required":["command"],"additionalProperties":false}}}), json!(["workspace","request_id","operations"])),
            tool("joppa_model_docs", "Read Joppa methodology and ASD-STE100 technical writing rules. Prefer English for new technical text. Read before creating a product tree.", json!({"section":{"type":"string","enum":["all","model","process","green","writing","objects"]}}), json!([])),
            tool("joppa_help", "Get operation shapes and writing rules. Read before authoring commands. Supply workspace to read schema questions. Check the title and description, then resend save_object with question_checked equal to the question.", json!({"workspace":{"type":"string"}}), json!([]))
        ]).as_array().into_iter().flatten()
            // A read-only grant sees only the tools that it can call.
            .filter(|t| access != store::Access::Read || t["name"].as_str().is_some_and(|n| READ_TOOLS.contains(&n)))
            .cloned().collect::<Vec<_>>()})),
        "tools/call" => call(pool, actor, access, msg, embedder)
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
            "op":{"type":"string","enum":joppa::commands::OPERATIONS.iter().map(|o| o.name).collect::<Vec<_>>(),"description":"Operation name. Read joppa_help.operations for its required fields and callers."},
            "question_checked":{"type":"string","description":"For save_object, use the exact configured type question after checking that the title and description answer it. Read joppa_help with workspace first."},
            "revision":{"description":"For save_object, archive_object, restore_object and set_tree_schema, send the revision you read. Zero creates a new object or changes the default schema. The server creates the next revision. For set_ac_tests, revision is optional: send the tests_revision you read, the document revision of the criterion, also when its list is empty."},
            "title":text(), "body":text(), "text":text(), "description":text(),
            "reason":text(), "contract":text(), "scope":text(), "value":text(),
            "acs":{"description":"For revise, AC objects contain text and description governed by the writing rules. For task, entries are AC identifiers.","items":{"properties":{"text":text(),"description":text()}}},
            "assessment":{"properties":{"reason":text()}},
            "checks":{"items":{"properties":{"expected":{"description":AUTHORING_TEXT,"items":text()},"tasks":{"description":"Optional Task bindings. Read process.origin_revision. Each entry uses address and origin_revision. Bind current Tasks in this verification context before execution.","type":"array","items":{"type":"object","properties":{"address":{"type":"string"},"origin_revision":{"type":"integer","minimum":1}},"required":["address","origin_revision"],"additionalProperties":false}}}}},
            "evidence":{"description":"For complete_task, use technical text or the required decision comment identifier. For block_task, give the failure facts, such as the failed test and commit. For unblock_task, give the facts that show the failure was outside the Task, such as the run. For result, observed and note follow the writing rules. Preserve exact facts, logs, code, identifiers, and quotations.","properties":{"observed":{"description":AUTHORING_TEXT,"items":text()},"note":text()}}
        }
    })
}
/// The only tools a read grant may call. Any other tool, known or not, is refused.
const READ_TOOLS: &[&str] = &[
    "joppa_list",
    "joppa_search",
    "joppa_read",
    "joppa_tests",
    "joppa_tree",
    "joppa_objects",
    "joppa_tasks",
    "joppa_timeline",
    "joppa_history",
    "joppa_help",
    "joppa_model_docs",
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
    match name {
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
        "joppa_tests" => {
            let workspace = field("workspace")?;
            if let Some(task) = a["task"].as_str() {
                return store::task_status(pool, workspace, actor, task).await;
            }
            // Without ac, MCP gives the same overview as HTTP GET /tests.
            let Some(ac) = a["ac"].as_str() else {
                return store::tests_overview(pool, workspace, actor).await;
            };
            let summary = a["summary"].as_bool().unwrap_or(false);
            let mut items = vec![];
            for target in ac.split(',').filter(|a| !a.is_empty()) {
                let item = store::criterion_tests(pool, workspace, actor, target).await?;
                items.push(if summary {
                    joppa::read_shapes::test_summary(item)
                } else {
                    item
                });
            }
            Ok(if items.len() == 1 {
                items.remove(0)
            } else {
                json!({"items": items})
            })
        }
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
        "joppa_objects" => {
            let revision = a
                .get("revision")
                .map(|v| {
                    v.as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or_else(|| anyhow::anyhow!("invalid object revision"))
                })
                .transpose()?;
            let limit = a
                .get("limit")
                .map(|v| v.as_u64().ok_or_else(|| anyhow::anyhow!("invalid limit")))
                .transpose()?
                .unwrap_or(50);
            let fields = joppa::read_shapes::fields_argument(&a["fields"])?;
            store::read_objects(
                pool,
                field("workspace")?,
                actor,
                &store::ObjectScope {
                    address: a["address"].as_str(),
                    parent: a["parent"].as_str(),
                    kind: a["kind"].as_str(),
                    revision,
                    history: a["history"].as_bool().unwrap_or(false),
                    limit: usize::try_from(limit)?,
                    cursor: a["cursor"].as_str(),
                    note_status: a["note_status"].as_str(),
                    related_to: a["related_to"].as_str(),
                    work_filter: a["work_filter"].as_str(),
                },
            )
            .await
            .map(|reply| joppa::read_shapes::keep_item_fields(reply, fields.as_deref()))
        }
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
        "joppa_timeline" => {
            let number = |k: &str, default: u64| {
                a.get(k)
                    .map(|v| {
                        v.as_u64()
                            .ok_or_else(|| anyhow::anyhow!("{k} must be a whole number"))
                    })
                    .transpose()
                    .map(|v| v.unwrap_or(default))
            };
            store::read_timeline(
                pool,
                field("workspace")?,
                actor,
                &store::TimelineScope {
                    after: number("after", 0)?,
                    limit: usize::try_from(number("limit", 50)?)?,
                    cursor: a["cursor"].as_str(),
                    order: a["order"].as_str(),
                },
            )
            .await
        }
        "joppa_record_results" => store::record_results(pool, field("workspace")?, actor, a).await,
        "joppa_history" => store::read_history(pool, field("workspace")?, actor, a).await,
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
        "joppa_model_docs" => {
            let section = a["section"].as_str().unwrap_or("all");
            let docs = [
                ("model", include_str!("../docs/model.md")),
                ("process", include_str!("../docs/process.md")),
                ("green", include_str!("../docs/green.md")),
                ("writing", WRITING_RULES),
                ("objects", include_str!("../docs/objects.md")),
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
    }
}
