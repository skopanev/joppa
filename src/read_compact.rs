//! The compact shape of MCP read replies. A list page gives at most PAGE short items, a
//! detail read keeps the stored document byte for byte and pages its comments and links,
//! and empty keys go away everywhere outside the authored document.
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

/// The page size of object lists, comments and links.
pub const PAGE: usize = 10;

/// Keys whose values are authored or replayed data. They stay exactly as stored.
const AUTHORED: [&str; 3] = ["document", "data", "command"];

/// The first path segments that a list item can give through `fields`.
const LIST_FIELDS: [&str; 19] = [
    "address",
    "kind",
    "id",
    "revision",
    "contract_revision",
    "parent",
    "title",
    "owner",
    "review_required",
    "work_ready",
    "item_id",
    "at",
    "description",
    "signal",
    "present",
    "note_status",
    "status",
    "process",
    "work_summary",
];

/// Refuse a `fields` path whose first segment is not a list item field.
pub fn check_fields(fields: &[String]) -> Result<()> {
    for field in fields {
        let head = field.split('.').next().unwrap_or_default();
        if !LIST_FIELDS.contains(&head) {
            bail!("unknown field {field}");
        }
    }
    Ok(())
}

/// The value at a dotted path, or null.
fn path(value: &Value, path: &str) -> Value {
    path.split('.')
        .try_fold(value, |v, key| v.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The status of an object: the label and the tone of its signal.
fn status(item: &Value) -> Option<Value> {
    let signal = &item["signal"];
    let mut out = Map::new();
    for key in ["label", "tone"] {
        if let Some(text) = signal[key].as_str().filter(|t| !t.is_empty()) {
            out.insert(key.into(), json!(text));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// One list item: address, title and the status tone. With `fields`, the address and each named
/// path, also when the value is empty.
pub fn list_item(item: &Value, fields: Option<&[String]>) -> Value {
    let mut out = Map::new();
    out.insert("address".into(), item["address"].clone());
    match fields {
        Some(fields) => {
            for field in fields {
                out.insert(field.clone(), path(item, field));
            }
        }
        None => {
            if let Some(title) = item["title"].as_str() {
                out.insert("title".into(), json!(title));
            }
            // The tone alone keeps 10 items under 1 KB; the detail read gives the label.
            if let Some(tone) = item["signal"]["tone"].as_str().filter(|t| !t.is_empty()) {
                out.insert("status".into(), json!(tone));
            }
            if item["present"] == false {
                out.insert("present".into(), json!(false));
            }
        }
    }
    Value::Object(out)
}

/// A list reply with compact items. Empty envelope keys go; `items` stays.
pub fn list(mut reply: Value, fields: Option<&[String]>) -> Value {
    let items: Vec<Value> = reply["items"]
        .as_array()
        .map(|items| items.iter().map(|i| list_item(i, fields)).collect())
        .unwrap_or_default();
    reply["items"] = Value::Array(vec![]);
    strip_empty(&mut reply);
    reply["items"] = Value::Array(items);
    reply
}

/// One page of `entries` from the offset in `cursor`: items, next_cursor and total.
pub fn page(entries: Vec<Value>, cursor: Option<&str>) -> Result<Value> {
    let start = match cursor {
        Some(c) => c
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("invalid cursor"))?,
        None => 0,
    };
    let total = entries.len();
    let items: Vec<Value> = entries.into_iter().skip(start).take(PAGE).collect();
    let mut out = json!({"items": items, "total": total});
    if start + PAGE < total {
        out["next_cursor"] = json!((start + PAGE).to_string());
    }
    Ok(out)
}

/// The comments of an annotation map as a list in time order, without duplicate keys.
fn comment_list(comments: &Value) -> Vec<Value> {
    let mut list: Vec<Value> = comments
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, c)| {
            let mut c = c.clone();
            c["id"] = json!(id);
            if c["author"] == c["actor"] {
                c.as_object_mut().map(|m| m.remove("author"));
            }
            if c["created_at"] == c["at"] {
                c.as_object_mut().map(|m| m.remove("created_at"));
            }
            c
        })
        .collect();
    list.sort_by(|a, b| {
        (a["at"].as_str(), a["id"].as_str()).cmp(&(b["at"].as_str(), b["id"].as_str()))
    });
    list
}

/// A detail reply: the stored document, the revision headers, one page of comments and of
/// titled links, one copy of the readiness assessment, and no empty keys outside document.
pub fn detail(
    mut reply: Value,
    revisions: Vec<Value>,
    comments_cursor: Option<&str>,
    links_cursor: Option<&str>,
) -> Result<Value> {
    let comments = comment_list(&reply["annotations"]["comments"]);
    if let Some(annotations) = reply["annotations"].as_object_mut() {
        annotations.remove("comments");
    }
    reply["comments"] = page(comments, comments_cursor)?;
    let links = reply["links"].as_array().cloned().unwrap_or_default();
    reply["links"] = page(links, links_cursor)?;
    if let Some(status) = status(&reply) {
        reply["status"] = status;
    }
    if reply["process"]["readiness_blocker"] == reply["process"]["assessment"] {
        reply["process"]
            .as_object_mut()
            .map(|p| p.remove("readiness_blocker"));
    }
    if let Some(map) = reply.as_object_mut() {
        map.remove("signal");
        map.remove("id");
    }
    reply["revisions"] = Value::Array(revisions);
    strip_empty(&mut reply);
    Ok(reply)
}

fn empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

/// Remove each key whose value is null, "", [] or {}, at any depth, but never inside the
/// authored keys. false and 0 stay.
pub fn strip_empty(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if !AUTHORED.contains(&key.as_str()) {
                    strip_empty(v);
                }
            }
            map.retain(|key, v| AUTHORED.contains(&key.as_str()) || !empty(v));
        }
        Value::Array(items) => items.iter_mut().for_each(strip_empty),
        _ => {}
    }
}

/// A non-list reply without empty keys; a top-level `items` list stays even when empty.
pub fn strip_reply(mut reply: Value) -> Value {
    let items = reply.get("items").cloned();
    strip_empty(&mut reply);
    if let Some(Value::Array(items)) = items
        && items.is_empty()
    {
        reply["items"] = json!([]);
    }
    reply
}
