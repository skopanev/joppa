//! The tools of tools/list, each with its parameter schema. Each property has a short
//! description. initialize gives the rules once, so the tool texts stay short.
use super::schemas::{self, CURSOR, WORKSPACE, text};
use serde_json::{Value, json};

/// The authoring rules of the text fields, once in the description of joppa_command.
const AUTHORING_TEXT: &str = "Text fields: each value holds at most the field_max_length of its type (default 1024 characters). Over 1 kb: prefer attach as a file. Prefer English. Follow ASD-STE100 in at least 80% of sentences in each text. Write an object reference as [Title](kind:id). Do not put link lists or progress notes in text fields: use relations and the note op. Read joppa_help section writing.";
/// The form of a note id, in joppa_command and joppa_batch.
const NOTE_ID: &str = "A note id has 1 to 120 ASCII letters, digits, \"-\", \"_\" or \".\".";
/// The text of each request_id parameter.
const REQUEST_ID: &str = "the same value only for an identical retry";

/// Every tool of tools/list.
pub(super) fn list() -> Vec<Value> {
    let w = || text(WORKSPACE);
    let mut tools = vec![
        tool(
            "joppa_list",
            "List your workspaces: items {id, title, hidden}. hidden: true means that you hid it from your web lists.",
            json!({"cursor":text(CURSOR)}),
            json!([]),
        ),
        tool(
            "joppa_search",
            "Find objects by words and meaning. A hit gives address, title, revision, score and excerpt.",
            schemas::search(),
            json!(["workspace", "query"]),
        ),
        tool(
            "joppa_tree",
            "Read the workspace schema: types, parents, roles, status rules, questions, references, typed fields, filters, relations and test_results. set_tree_schema with schema.revision changes it: put_types, remove_types, put_relations and remove_relations.",
            json!({"workspace":w(),"revision":{"type":"integer","minimum":0,"description":"one schema revision"}}),
            json!(["workspace"]),
        ),
        tool(
            "joppa_objects",
            "Read objects. Without address: a page of items {address, title, status, assignee}. With address: one object with revision, parent, data, status, process, relations and relation_groups; 2 to 25 addresses give {items}. relations_of gives edges in pages. A reply over 256 KB is refused.",
            schemas::objects(),
            json!([]),
        ),
        tool(
            "joppa_record_results",
            "Record the test results of one commit. Name each test by test:<id> or its public item ID: it binds that one node. A title binds every present test node with that title. The latest record decides a test. The reply gives unbound: the tests with no present test node. unbound does not check under which criterion a test node stands. A full-run record needs unbound: [] before a merge.",
            json!({"workspace":w(),"commit":text("lowercase git sha"),"results":{"type":"array","minItems":1,"description":"[{test, status, ...fields}]","items":{"type":"object","properties":{"test":text("test:<id>, a public item ID or a title"),"status":text("a test_results status of joppa_tree")},"required":["test","status"]}}}),
            json!(["workspace", "commit", "results"]),
        ),
        tool(
            "joppa_history",
            "Read the History of a workspace: one entry for each change, open until a member marks it processed.",
            json!({"workspace":w(),"limit":{"type":"integer","minimum":1,"maximum":25,"description":"1 to 25, default 25"},"cursor":text(CURSOR),"order":{"type":"string","enum":["oldest","newest"],"description":"default oldest"},"after":{"type":"integer","minimum":0,"description":"only the entries after this position"},"open":{"type":"boolean","description":"only open entries; next_item names the oldest"}}),
            json!(["workspace"]),
        ),
        tool(
            "joppa_history_mark",
            "Mark one open History entry processed. The server sets actor and time.",
            json!({"workspace":w(),"position":{"type":"integer","minimum":1,"description":"History position"},"reason":text("short reason"),"nodes":{"type":"array","maxItems":15,"items":{"type":"string"},"description":"up to 15 nodes that the work changed"}}),
            json!(["workspace", "position", "reason"]),
        ),
        tool(
            "joppa_command",
            &format!(
                "Append one operation. Read joppa_help with op for its fields. On an update, send the revision you read and only the changed data fields. Link objects only with the relations map of save_object. Add findings as comments: {{\"op\":\"note\",\"target\":\"kind:id\",\"id\":\"<unique>\",\"text\":\"...\"}}. {NOTE_ID} {AUTHORING_TEXT}"
            ),
            json!({"workspace":w(),"request_id":text(REQUEST_ID),"command":command_schema()}),
            json!(["workspace", "request_id", "command"]),
        ),
        tool(
            "joppa_batch",
            &format!(
                "Append up to 100 operations atomically, by the rules of joppa_command. A later command names an earlier result as {{\"$ref\":\"label/id\"}} or {{\"$ref\":\"label/address\"}}. {NOTE_ID}"
            ),
            json!({"workspace":w(),"request_id":text(REQUEST_ID),"defaults":{"type":"object","description":"save_object fields for each save_object that omits them"},"operations":{"type":"array","minItems":1,"maxItems":100,"description":"the operations in order","items":{"type":"object","properties":{"ref":text("label of this result"),"command":{"type":"object","description":"One operation object, by the rules of joppa_command.command."}},"required":["command"],"additionalProperties":false}}}),
            json!(["workspace", "request_id", "operations"]),
        ),
        tool(
            "joppa_help",
            "Read the operation shapes and the rules. Without section, op or kind: the table of contents.",
            json!({"workspace":text("workspace key or alias, for examples"),"section":{"type":"string","enum":["operations","writing","objects","search","process","examples"],"description":"one section"},"op":text("one operation: its fields and callers"),"kind":text("the example of one type")}),
            json!([]),
        ),
    ];
    tools.extend(super::files::tools());
    tools
}

/// One tool. Every tool takes `alias`: initialize describes it once.
pub(crate) fn tool(name: &str, description: &str, mut properties: Value, required: Value) -> Value {
    properties["alias"] = text("your agent name, such as p1");
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}

/// The command of joppa_command: the fields that help does not repeat for each operation.
fn command_schema() -> Value {
    let rules = || json!({"description":"by the text rules above"});
    let ops: Vec<&str> = joppa::commands::mcp_operations().map(|o| o.name).collect();
    json!({
        "type":"object",
        "description":"Fields by op. An object field takes an address or a public item ID.",
        "properties":{
            "op":{"type":"string","enum":ops,"description":"joppa_help with op gives its fields"},
            "revision":{"description":"the revision you read; 0 creates"},
            "relations":{"type":"object","description":"save_object: key -> {edges: [{to}]}; null removes a key"},
            "clean":{"type":"boolean","description":"true replaces data whole"},
            "data":{"type":"object","description":"data.owner: the member ID of a human owner, required on every save"},
            "title":{"description":"one statement, at most 128 characters"},
            "body":rules(), "text":rules(), "reason":rules(), "value":rules()
        }
    })
}
