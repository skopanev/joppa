//! The joppa_objects reads by address (requirement:read-contract): one object, or 2 to 25
//! objects as items that name their addresses. A current read runs in one snapshot and loads
//! only the facts that its fields need (store::read_cards). A revision and the history read
//! one address with the full object read.
use anyhow::ensure;
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, store};
use serde_json::{Value, json};

/// The addresses of a read: one address, or up to one page of addresses separated by commas.
pub(super) fn addresses(value: &Value) -> anyhow::Result<Option<Vec<&str>>> {
    let Some(text) = value.as_str() else {
        return Ok(None);
    };
    let list: Vec<&str> = text.split(',').map(str::trim).collect();
    ensure!(
        list.iter().all(|a| !a.is_empty()),
        "empty address in the address list"
    );
    ensure!(
        list.len() <= rc::PAGE,
        "at most {} addresses in one call",
        rc::PAGE
    );
    Ok(Some(list))
}

/// The read of `addresses`: the object itself for one address, else `{items}` in the order
/// of the request, each item with its address.
pub(super) async fn read<'a>(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    a: &'a Value,
    addresses: &[&'a str],
    fields: Option<&[String]>,
    scope: impl Fn(Option<&'a str>) -> store::ObjectScope<'a>,
) -> anyhow::Result<Value> {
    let (history, revision) = (a["history"] == true, a["revision"].as_u64());
    let many = addresses.len() > 1;
    let cursor = a["comments_cursor"].as_str();
    ensure!(
        !many || !(history || revision.is_some() || cursor.is_some()),
        "history, revision and comments_cursor read one address"
    );
    ensure!(
        fields.is_none() || !history,
        "fields reads one revision; history gives the revision headers"
    );
    if history || revision.is_some() {
        let asked = addresses[0];
        let reply = store::read_objects(pool, workspace, actor, &scope(Some(asked))).await?;
        return match fields {
            Some(fields) => named(pool, workspace, actor, reply, asked, fields, cursor).await,
            None if history => history_page(reply, asked, a["cursor"].as_str()),
            None => Ok(rc::revision(reply, asked)),
        };
    }
    let list = scope(None);
    ensure!(
        list.filter == joppa::node_filter::NodeFilter::default()
            && list.cursor.is_none()
            && list.note_status.is_none()
            && list.related_to.is_none(),
        "address cannot be combined with list filters"
    );
    // A comment page without fields gives only the comments.
    let comments = ["comments".to_owned()];
    let fields = fields.or(cursor.map(|_| &comments[..]));
    if let Some(fields) = fields {
        super::check(fields, true, &[])?;
    }
    let replies = store::read_cards(pool, workspace, actor, addresses, rc::card_needs(fields));
    let mut items = Vec::with_capacity(addresses.len());
    for (reply, asked) in replies.await?.into_iter().zip(addresses) {
        let address = reply["address"].clone();
        let mut item = match fields {
            Some(fields) => {
                let detail = match fields.iter().any(|f| detail_key(f)) {
                    true => Some(rc::detail_keys(reply.clone(), asked, cursor)?),
                    false => None,
                };
                rc::object_fields(&reply, detail.as_ref(), fields)
            }
            None => rc::detail(reply, asked, None)?,
        };
        // Each item of a read of more addresses names its object.
        if many {
            item["address"] = address;
        }
        items.push(item);
    }
    Ok(if many {
        json!({"items": items})
    } else {
        items.remove(0)
    })
}

fn detail_key(field: &str) -> bool {
    rc::DETAIL_FIELDS.contains(&rc::head(field))
}

/// The named fields of one revision: list fields, detail keys and document fields. The
/// comments are the page at `comments_cursor`. The state fields stay current.
async fn named(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    mut reply: Value,
    asked: &str,
    fields: &[String],
    comments_cursor: Option<&str>,
) -> anyhow::Result<Value> {
    let data = &reply["document"]["data"];
    super::check(fields, true, &[data])?;
    if fields.iter().any(|f| rc::head(f) == "note_status") {
        let inbox = super::inbox_kind(pool, workspace, actor, reply["kind"].as_str()).await?;
        reply["inbox"] = json!(inbox);
    }
    let detail = match fields.iter().any(|f| detail_key(f)) {
        true => Some(rc::detail_keys(reply.clone(), asked, comments_cursor)?),
        false => None,
    };
    // The number of present children is the total of their list.
    if fields.iter().any(|f| f == "children") {
        let filter = joppa::node_filter::NodeFilter {
            depth: Some(1),
            ..Default::default()
        }
        .parent(reply["address"].as_str().unwrap_or_default());
        let list = store::compact_list(pool, workspace, actor, &filter, 1, None).await?;
        reply["children"] = list["total"].clone();
    }
    Ok(rc::object_fields(&reply, detail.as_ref(), fields))
}

/// The revision headers in pages of 25, oldest first: items, total and next_cursor.
fn history_page(mut reply: Value, asked: &str, cursor: Option<&str>) -> anyhow::Result<Value> {
    let revisions = match reply["revisions"].take() {
        Value::Array(revisions) => revisions,
        _ => vec![],
    };
    reply.as_object_mut().map(|m| m.remove("revisions"));
    let page = rc::page(revisions, cursor)?;
    let mut out = rc::headers(reply, asked);
    for key in ["items", "total", "next_cursor"] {
        if !page[key].is_null() {
            out[key] = page[key].clone();
        }
    }
    Ok(out)
}
