//! The `fields` of MCP object reads (requirement:read-contract). A path names a list field, a
//! key of the detail read (only for a read by address) or a document field: each other bare
//! name such as `tags` gives `data.tags`, or null when the data has no such field.
use crate::store::CardNeeds;
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

/// The first path segments that a list item gives itself. `status` is the status tone, and
/// `relations` gives the stored relations of the item. A read by address gives its detail
/// relations.
pub const LIST_FIELDS: [&str; 18] = [
    "address",
    "data",
    "kind",
    "id",
    "revision",
    "parent",
    "title",
    "owner",
    "at",
    "body",
    "signal",
    "present",
    "note_status",
    "status",
    "process",
    "children",
    "child_counts",
    "relations",
];

/// The keys of a detail read that a list item does not give. `comments_count` and
/// `files_count` are system fields: a data field of that name does not change them.
pub const DETAIL_FIELDS: [&str; 13] = [
    "relations",
    "comments",
    "comments_count",
    "files",
    "files_count",
    "references",
    "relation_groups",
    "remote_links",
    "remote_review_required",
    "remote_links_revision",
    "child_workspace",
    "annotations",
    "result",
];

/// The first segment of a path.
pub fn head(field: &str) -> &str {
    field.split('.').next().unwrap_or_default()
}

/// Whether a read must load the document data for `fields`.
pub fn needs_data(fields: &[String]) -> bool {
    fields.iter().any(|f| {
        let head = head(f);
        head == "data" || head == "relations" || !LIST_FIELDS.contains(&head)
    })
}

/// The facts that a read by address loads for `fields` (tac:jogrtcra). Without `fields`, the
/// MCP card. A list field reads the object row only.
pub fn card_needs(fields: Option<&[String]>) -> CardNeeds {
    let Some(fields) = fields else {
        return CardNeeds::CARD;
    };
    let mut needs = CardNeeds::default();
    for field in fields {
        match head(field) {
            "data" => needs.document = true,
            "relations" => (needs.document, needs.edges) = (true, true),
            "comments" | "files" | "annotations" => needs.notes = true,
            "comments_count" | "files_count" => needs.counts = true,
            "relation_groups" => needs.groups = true,
            "references" => (needs.document, needs.references) = (true, true),
            "remote_links" | "remote_review_required" | "remote_links_revision" => {
                needs.remote = true
            }
            "child_workspace" => (needs.document, needs.remote) = (true, true),
            "children" => needs.children = true,
            "result" => needs.result = true,
            "note_status" => needs.inbox = true,
            // A bare name reads its data field.
            head if !LIST_FIELDS.contains(&head) && !DETAIL_FIELDS.contains(&head) => {
                needs.document = true
            }
            _ => {}
        }
    }
    needs
}

/// Refuse the first path with an older text field or with a detail key that `known` does not
/// accept. The text of every object type is `body`, so `description` and `contract` are
/// refused, also under `data`. `known` accepts the detail keys of a read by address and each
/// key of a returned document, so a detail key on a list read is refused. Each other bare name
/// reads `data.<name>`, and an absent field gives null.
pub fn check_fields(fields: &[String], known: impl Fn(&str) -> bool) -> Result<()> {
    for field in fields {
        let head = head(field);
        let text = field.strip_prefix("data.").map_or(head, self::head);
        if let Some(old) = crate::body_migration::OLD_FIELDS
            .iter()
            .find(|f| **f == text)
        {
            bail!(crate::body_migration::refusal(old));
        }
        if DETAIL_FIELDS.contains(&head) && !LIST_FIELDS.contains(&head) && !known(head) {
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
        Some(detail) if DETAIL_FIELDS.contains(&head) => path(detail, field),
        _ if LIST_FIELDS.contains(&head) => path(item, field),
        _ => path(&item["data"], field),
    }
}

/// An object read by its address with `fields`: its address and each named path. The list
/// fields have the values of the list item of the object: `children` and `child_counts` too.
/// `inbox` marks an object of an Inbox type.
pub fn object_fields(reply: &Value, detail: Option<&Value>, fields: &[String]) -> Value {
    let document = &reply["document"];
    let data = &document["data"];
    // A read of an older revision gives its number. The state fields stay current.
    let revision = match &document["revision"] {
        Value::Null => reply["current_revision"].clone(),
        number => number.clone(),
    };
    let inbox = reply["inbox"] == true;
    let note_status = inbox.then(|| data["status"].as_str().unwrap_or("unprocessed"));
    let item = json!({"address": reply["address"], "kind": reply["kind"], "id": reply["id"],
        "revision": revision,
        "parent": document["parent"], "data": data, "title": data["title"],
        "owner": data["owner"],
        "at": document["at"], "body": data["body"],
        "signal": reply["signal"], "present": document["present"] != false,
        "note_status": note_status, "process": reply["process"],
        "status": crate::read_compact::status(reply), "children": reply["children"],
        "child_counts": reply["child_counts"]});
    let mut out = Map::new();
    out.insert("address".into(), item["address"].clone());
    for field in fields {
        out.insert(field.clone(), value(&item, detail, field));
    }
    Value::Object(out)
}

#[cfg(test)]
#[path = "read_fields_tests.rs"]
mod tests;
