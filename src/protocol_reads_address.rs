//! The joppa_objects reads by address (requirement:read-contract): one object, or 2 to 25
//! objects as items that name their addresses.
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
    ensure!(
        !many || !(history || revision.is_some() || a["comments_cursor"].is_string()),
        "history, revision and comments_cursor read one address"
    );
    ensure!(
        fields.is_none() || !(history || revision.is_some()),
        "fields reads the current revision"
    );
    let mut items = Vec::with_capacity(addresses.len());
    for asked in addresses {
        let reply = store::read_objects(pool, workspace, actor, &scope(Some(asked))).await?;
        let address = reply["address"].clone();
        let mut item = match fields {
            Some(fields) => named(pool, workspace, actor, reply, asked, fields).await?,
            None if history => history_page(reply, asked, a["cursor"].as_str())?,
            None if revision.is_some() => rc::revision(reply, asked),
            None => rc::detail(reply, asked, a["comments_cursor"].as_str())?,
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

/// The named fields of one object: list fields, detail keys and document fields.
async fn named(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    reply: Value,
    asked: &str,
    fields: &[String],
) -> anyhow::Result<Value> {
    let data = &reply["document"]["data"];
    super::check(pool, workspace, actor, fields, true, &[data]).await?;
    let inbox = fields.iter().any(|f| rc::head(f) == "note_status")
        && super::inbox_kind(pool, workspace, actor, reply["kind"].as_str()).await?;
    let detail = match fields
        .iter()
        .any(|f| rc::DETAIL_FIELDS.contains(&rc::head(f)))
    {
        true => Some(rc::detail(reply.clone(), asked, None)?),
        false => None,
    };
    // The number of present children is the total of their list.
    let children = match fields.iter().any(|f| f == "children") {
        true => {
            let address = reply["address"].as_str();
            let list = store::compact_list(pool, workspace, actor, address, None, 1, None).await?;
            list["total"].as_i64()
        }
        false => None,
    };
    Ok(rc::object_fields(
        &reply,
        detail.as_ref(),
        fields,
        inbox,
        children,
    ))
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
