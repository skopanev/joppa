//! The `fields` of MCP object reads (requirement:read-contract). A path names a list field, a
//! key of the detail read (only for a read by address) or a document field: a bare document
//! field name such as `body` gives `data.body`.
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

/// The first path segments that a list item gives itself. `status` is the status tone.
pub const LIST_FIELDS: [&str; 19] = [
    "address",
    "data",
    "kind",
    "id",
    "revision",
    "contract_revision",
    "parent",
    "title",
    "owner",
    "review_required",
    "work_ready",
    "at",
    "description",
    "signal",
    "present",
    "note_status",
    "status",
    "process",
    "work_summary",
];

/// The keys of a detail read that a list item does not give.
pub const DETAIL_FIELDS: [&str; 12] = [
    "links",
    "comments",
    "files",
    "references",
    "link_groups",
    "remote_links",
    "remote_review_required",
    "remote_links_revision",
    "child_workspace",
    "verification_summary",
    "question",
    "annotations",
];

/// The document fields of the object roles. The tree schema adds its references and
/// required fields, and each key of a returned document counts too.
pub const DOCUMENT_FIELDS: [&str; 20] = [
    "archived",
    "author",
    "answers",
    "before_launch",
    "body",
    "consumer",
    "consumers",
    "contract",
    "decision",
    "decision_required",
    "depends_on",
    "note",
    "outcome",
    "projects",
    "purpose",
    "reason",
    "repository",
    "requirement",
    "tests",
    "text",
];

/// The first segment of a path.
pub fn head(field: &str) -> &str {
    field.split('.').next().unwrap_or_default()
}

/// Whether a read must load the document data for `fields`.
pub fn needs_data(fields: &[String]) -> bool {
    fields.iter().any(|f| {
        let head = head(f);
        head == "data" || !LIST_FIELDS.contains(&head)
    })
}

/// Refuse the first path whose first segment `known` does not accept.
pub fn check_fields(fields: &[String], known: impl Fn(&str) -> bool) -> Result<()> {
    for field in fields {
        let head = head(field);
        if !LIST_FIELDS.contains(&head) && !DOCUMENT_FIELDS.contains(&head) && !known(head) {
            bail!("unknown field {field}");
        }
    }
    Ok(())
}

/// The value at a dotted path, or null.
pub(crate) fn path(value: &Value, path: &str) -> Value {
    path.split('.')
        .try_fold(value, |v, key| v.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The value of one path: a list field of `item`, a key of `detail`, or a path in `data`.
pub fn value(item: &Value, detail: Option<&Value>, field: &str) -> Value {
    let head = head(field);
    match detail {
        // `status` is the tone, as in a plain list item; `note_status` names the Inbox state.
        _ if field == "status" => item["signal"]["tone"].clone(),
        _ if LIST_FIELDS.contains(&head) => path(item, field),
        Some(detail) if DETAIL_FIELDS.contains(&head) => path(detail, field),
        _ => path(&item["data"], field),
    }
}

/// An object read by its address with `fields`: its address and each named path. The list
/// fields have the values of the list item of the object.
pub fn object_fields(
    reply: &Value,
    detail: Option<&Value>,
    fields: &[String],
    inbox: bool,
) -> Value {
    let document = &reply["document"];
    let data = &document["data"];
    // A list cuts the description to a preview of 300 characters.
    let description = data["description"]
        .as_str()
        .map(|text| json!(text.chars().take(300).collect::<String>()));
    let note_status = inbox.then(|| data["status"].as_str().unwrap_or("unprocessed"));
    let item = json!({"address": reply["address"], "kind": reply["kind"], "id": reply["id"],
        "revision": reply["current_revision"], "contract_revision": reply["contract_revision"],
        "parent": document["parent"], "data": data, "title": data["title"],
        "owner": data["owner"], "review_required": reply["review_required"],
        "work_ready": reply["work_ready"], "at": document["at"], "description": description,
        "signal": reply["signal"], "present": document["present"] != false,
        "note_status": note_status, "process": reply["process"],
        "status": crate::read_compact::status(reply)});
    let mut out = Map::new();
    out.insert("address".into(), item["address"].clone());
    for field in fields {
        out.insert(field.clone(), value(&item, detail, field));
    }
    Value::Object(out)
}
