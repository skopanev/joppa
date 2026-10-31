//! The lean MCP replies (requirement:read-contract): no echo of the request, no workspace,
//! no snapshot position and no default values. The HTTP routes keep their own shapes.
use serde_json::{Map, Value, json};

/// The tools whose replies give no workspace, no position and no echo of the address.
const READS: [&str; 9] = [
    "joppa_objects",
    "joppa_search",
    "joppa_tasks",
    "joppa_timeline",
    "joppa_history",
    "joppa_tests",
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

/// The largest text of an MCP tool reply: 256 KiB.
pub const REPLY_MAX: usize = 262_144;

/// The text of a tool reply. A larger text is refused, so a client never gets a cut reply.
pub fn text(reply: &Value) -> anyhow::Result<String> {
    let text = reply.to_string();
    anyhow::ensure!(
        text.len() <= REPLY_MAX,
        "reply exceeds 256 KB; narrow the request with fields, fewer addresses or a smaller limit"
    );
    Ok(text)
}

/// The lean reply of `tool` for the request `args`.
pub fn lean(tool: &str, args: &Value, mut reply: Value) -> Value {
    if READS.contains(&tool) {
        drop_keys(&mut reply, &["workspace", "position"]);
        for key in ["address", "links_of", "ac", "task"] {
            drop_value(&mut reply, &["address"], &args[key]);
        }
        drop_value(&mut reply, &["target"], &args["target"]);
    }
    match tool {
        "joppa_objects" if args["links_of"].is_string() => each(&mut reply, "items", |item| {
            drop_value(item, &["present"], &json!(true));
            drop_value(item, &["blocking"], &json!(false));
            drop_value(item, &["relation"], &json!("related"));
        }),
        "joppa_search" => search(&mut reply),
        "joppa_tasks" => each(&mut reply, "items", |item| {
            drop_value(item, &["claimable", "review_required"], &json!(false));
            drop_keys(item, &["created_at"]);
        }),
        "joppa_tests" => {
            tests(&mut reply);
            each(&mut reply, "items", tests);
        }
        "joppa_tree" => tree(&mut reply, args),
        "joppa_help" => {
            if let Some(schema) = reply.get_mut("tree") {
                drop_keys(schema, &["workspace", "position"]);
                tree(schema, args);
            }
        }
        "joppa_command" | "joppa_batch" => {
            match reply.get_mut("results").and_then(Value::as_array_mut) {
                Some(results) => results.iter_mut().for_each(write),
                None => write(&mut reply),
            }
            drop_value(&mut reply, &["replayed"], &json!(false));
        }
        "joppa_attach" | "joppa_record_results" => {
            drop_value(&mut reply, &["replayed"], &json!(false));
        }
        _ => {}
    }
    reply
}

/// A search page: no index counters, and each hit names itself by its address.
fn search(reply: &mut Value) {
    drop_keys(reply, &["embedded", "indexed", "mode", "semantic_status"]);
    each(reply, "items", |item| {
        if let (Some(kind), Some(id)) = (item["kind"].as_str(), item["id"].as_str()) {
            item["address"] = json!(format!("{kind}:{id}"));
        }
        drop_keys(item, &["kind", "id"]);
        drop_value(item, &["archived", "historical", "exact_id"], &json!(false));
    });
}

/// One criterion of joppa_tests: no default list flags, no list author, and a test status
/// name only when it differs from the status.
fn tests(criterion: &mut Value) {
    drop_value(criterion, &["tests_draft"], &json!(false));
    drop_keys(criterion, &["tests_saved_by"]);
    each(criterion, "tests", |test| {
        let status = test["status"].clone();
        drop_value(test, &["status_name"], &status);
    });
}

/// The tree schema: one revision, no write header, no copy of the status rules of the types,
/// and no false type flags. The workspace key shows only when the request named an alias.
fn tree(reply: &mut Value, args: &Value) {
    drop_keys(reply, &["current_revision"]);
    drop_value(reply, &["workspace_key"], &args["workspace"]);
    let Some(schema) = reply.get_mut("schema") else {
        return;
    };
    drop_keys(
        schema,
        &[
            "actor",
            "actor_type",
            "at",
            "position",
            "reason",
            "status_rules",
        ],
    );
    if let Some(types) = schema.get_mut("types").and_then(Value::as_object_mut) {
        for kind in types.values_mut() {
            drop_value(kind, &["inbox", "launch_blocker", "root"], &json!(false));
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
    fn a_reply_text_above_256_kib_is_refused() {
        let at = json!("x".repeat(REPLY_MAX - 2));
        assert_eq!(text(&at).map(|t| t.len()).ok(), Some(REPLY_MAX));
        let above = json!("x".repeat(REPLY_MAX - 1));
        let error = text(&above).unwrap_err().to_string();
        assert!(error.starts_with("reply exceeds 256 KB; narrow the request"));
    }
}
