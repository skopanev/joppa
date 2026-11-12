//! The lean MCP replies (requirement:read-contract): no echo of the request, no workspace,
//! no snapshot position and no default values. The HTTP routes keep their own shapes.
use serde_json::{Map, Value, json};

/// The tools whose replies give no workspace, no position and no echo of the address.
const READS: [&str; 6] = [
    "joppa_objects",
    "joppa_search",
    "joppa_history",
    "joppa_tree",
    "joppa_attach",
    "joppa_attachments",
];

fn object(value: &mut Value) -> Option<&mut Map<String, Value>> {
    value.as_object_mut()
}

/// Remove each of `keys` whose value is `same`.
fn drop_value(value: &mut Value, keys: &[&str], same: &Value) {
    if let Some(map) = object(value) {
        map.retain(|key, v| !(keys.contains(&key.as_str()) && v == same));
    }
}

fn drop_keys(value: &mut Value, keys: &[&str]) {
    if let Some(map) = object(value) {
        map.retain(|key, _| !keys.contains(&key.as_str()));
    }
}

/// Each item of the list at `key`.
fn each(value: &mut Value, key: &str, mut f: impl FnMut(&mut Value)) {
    if let Some(items) = value.get_mut(key).and_then(Value::as_array_mut) {
        items.iter_mut().for_each(&mut f);
    }
}

/// The largest text of an MCP tool reply with a list of items: 256 KiB.
pub const REPLY_MAX: usize = 262_144;

/// The text of a tool reply. A reply with an `items` or a `files` list above REPLY_MAX is
/// refused, so a client never gets a cut list. One object without such a list comes in full:
/// one field can hold 256 KiB.
pub fn text(reply: &Value) -> anyhow::Result<String> {
    let text = reply.to_string();
    let list = reply["items"].is_array() || reply["files"].is_array();
    anyhow::ensure!(
        text.len() <= REPLY_MAX || !list,
        "reply exceeds 256 KB; narrow the request with fields, fewer addresses or a smaller limit"
    );
    Ok(text)
}

/// The lean reply of `tool` for the request `args`.
pub fn lean(tool: &str, args: &Value, mut reply: Value) -> Value {
    if READS.contains(&tool) {
        drop_keys(&mut reply, &["workspace", "position"]);
        for key in ["address", "relations_of", "ac", "task"] {
            drop_value(&mut reply, &["address"], &args[key]);
        }
        drop_value(&mut reply, &["target"], &args["target"]);
    }
    match tool {
        "joppa_search" => search(&mut reply, args),
        "joppa_tree" => tree(&mut reply, args),
        "joppa_command" | "joppa_batch" => {
            match reply.get_mut("results").and_then(Value::as_array_mut) {
                Some(results) => results.iter_mut().for_each(write),
                None => write(&mut reply),
            }
            drop_value(&mut reply, &["replayed"], &json!(false));
        }
        "joppa_attach" => drop_value(&mut reply, &["replayed"], &json!(false)),
        // A record gives no commit and no actor; `unbound` stays, also when it is empty.
        "joppa_record_results" => {
            drop_value(&mut reply, &["replayed"], &json!(false));
            drop_keys(&mut reply, &["commit", "actor"]);
        }
        // A mark keeps its alias and nodes, and gives no reason (ac:flg87alq).
        "joppa_history_mark" => drop_keys(&mut reply, &["reason"]),
        _ => {}
    }
    reply
}

/// The keys that a search hit gives for `fields`; the address always comes.
pub const SEARCH_FIELDS: [&str; 4] = ["title", "excerpt", "revision", "status"];

/// Refuse a search `fields` entry outside SEARCH_FIELDS, before the search runs.
pub fn search_fields(args: &Value) -> anyhow::Result<Option<Vec<String>>> {
    let Some(fields) = crate::read_shapes::fields_argument(&args["fields"])? else {
        return Ok(None);
    };
    if let Some(field) = fields.iter().find(|f| !SEARCH_FIELDS.contains(&f.as_str())) {
        anyhow::bail!(
            "unknown search field {field}; search takes title, excerpt, revision and status"
        );
    }
    Ok(Some(fields))
}

/// A search page: no index counters, and each hit names itself by its address. A hit has no
/// score. With `fields` it gives only its address and the named keys; without them it gives
/// no status.
fn search(reply: &mut Value, args: &Value) {
    drop_keys(reply, &["embedded", "indexed", "mode", "semantic_status"]);
    let fields = search_fields(args).ok().flatten();
    each(reply, "items", |item| {
        if let (Some(kind), Some(id)) = (item["kind"].as_str(), item["id"].as_str()) {
            item["address"] = json!(format!("{kind}:{id}"));
        }
        drop_keys(item, &["kind", "id", "score"]);
        drop_value(item, &["archived", "historical", "exact_id"], &json!(false));
        match &fields {
            Some(fields) => {
                if let Some(map) = object(item) {
                    map.retain(|key, _| {
                        ["address", "workspace"].contains(&key.as_str()) || fields.contains(key)
                    });
                }
            }
            None => drop_keys(item, &["status"]),
        }
    });
}

/// The tree schema: one revision, no write header, no copy of the status rules of the types,
/// no colours and no false type flags (D2). `priorities` is the list that applies (D3). A type
/// gives no board field, no board flag of a typed field and no default title limit. The
/// workspace key shows only when the request named an alias.
fn tree(reply: &mut Value, args: &Value) {
    drop_keys(reply, &["current_revision"]);
    drop_value(reply, &["workspace_key"], &args["workspace"]);
    let Some(schema) = reply.get_mut("schema") else {
        return;
    };
    drop_keys(
        schema,
        &["actor", "at", "position", "reason", "status_rules"],
    );
    drop_keys(schema, &["status_colors", "statuses"]);
    if let Some(map) = object(schema)
        && let Some(levels) = map.remove("priority_levels")
    {
        map.insert("priorities".into(), levels);
    }
    let default_title = json!(crate::objects::TITLE_LENGTH_DEFAULT);
    if let Some(types) = schema.get_mut("types").and_then(Value::as_object_mut) {
        for kind in types.values_mut() {
            drop_value(kind, &["inbox", "root"], &json!(false));
            drop_keys(kind, &crate::schema_nodes::UI_FIELDS);
            let fields = kind.get_mut("fields").and_then(Value::as_object_mut);
            fields
                .into_iter()
                .flatten()
                .for_each(|(_, f)| drop_keys(f, &["board"]));
            let title = ["title_max_length", "title_preview_length"];
            drop_value(kind, &title, &default_title);
        }
    }
}

/// One write result: when `object` names the saved object and its revision, `command` gives
/// only its op.
fn write(result: &mut Value) {
    let saved = &result["object"];
    if saved["address"].is_string() && saved["revision"].is_number() {
        result["command"] = json!({"op": result["command"]["op"]});
    }
    drop_value(result, &["replayed"], &json!(false));
}

#[cfg(test)]
mod reply_ceiling {
    use super::*;

    #[test]
    fn a_list_reply_text_above_256_kib_is_refused() {
        // {"items":[""]} has 14 bytes around the text.
        let at = json!({"items": ["x".repeat(REPLY_MAX - 14)]});
        assert_eq!(text(&at).map(|t| t.len()).ok(), Some(REPLY_MAX));
        let above = json!({"items": ["x".repeat(REPLY_MAX - 13)]});
        let error = text(&above).unwrap_err().to_string();
        assert!(error.starts_with("reply exceeds 256 KB; narrow the request"));
        // One object has no items and comes in full.
        let object = json!({"data": {"body": "x".repeat(REPLY_MAX)}});
        assert!(text(&object).is_ok_and(|t| t.len() > REPLY_MAX));
    }

    // ac:flg87alq, ac:read-mcp-echoes
    #[test]
    fn a_record_and_a_mark_reply_echo_no_commit_actor_or_reason() {
        let record = json!({"record":7,"commit":"a".repeat(40),"tests":1,"replayed":false,
            "actor":"bot","position":12,"unbound":[]});
        let reply = lean("joppa_record_results", &json!({}), record);
        let lean_record = json!({"record":7,"tests":1,"position":12,"unbound":[]});
        assert_eq!(reply, lean_record);
        let retry = json!({"record":7,"commit":"a","tests":1,"replayed":true,"actor":"bot",
            "position":13,"unbound":[]});
        let reply = lean("joppa_record_results", &json!({}), retry);
        let replayed = json!({"record":7,"tests":1,"replayed":true,"position":13,"unbound":[]});
        assert_eq!(reply, replayed);
        let mark = json!({"position":5,"status":"processed","actor":"bot","at":"t",
            "reason":"Read","nodes":["task:t"],"alias":"p5"});
        let reply = lean("joppa_history_mark", &json!({"reason":"Read"}), mark);
        let kept = json!({"position":5,"status":"processed","actor":"bot","at":"t",
            "nodes":["task:t"],"alias":"p5"});
        assert_eq!(reply, kept);
    }

    #[test]
    fn a_files_reply_text_above_256_kib_is_refused() {
        // {"files":[""]} has 14 bytes around the text.
        let at = json!({"files": ["x".repeat(REPLY_MAX - 14)]});
        assert_eq!(text(&at).map(|t| t.len()).ok(), Some(REPLY_MAX));
        let above = json!({"files": ["x".repeat(REPLY_MAX - 13)]});
        let error = text(&above).unwrap_err().to_string();
        assert!(error.starts_with("reply exceeds 256 KB; narrow the request"));
    }
}
