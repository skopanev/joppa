//! The compact MCP read of joppa_objects (requirement:read-contract).
use anyhow::Context;
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, read_shapes, store};
use serde_json::{Value, json};

pub async fn compact(pool: &Pool, actor: &str, _name: &str, a: &Value) -> anyhow::Result<Value> {
    let keys = ["address", "parent", "related_to", "relations_of"];
    let resolved = with_addresses(pool, actor, a, &keys).await?;
    let mut reply = objects(pool, actor, &resolved).await?;
    // A read of one object by its public item ID names the address that it resolved.
    let address = &resolved["address"];
    if a["address"] != *address && reply.get("items").is_none() && reply.is_object() {
        reply["address"] = address.clone();
    }
    Ok(reply)
}

/// The request with each public item ID in `keys`, also in a list separated by commas,
/// replaced by the address of its object. A request without a workspace reads the workspace
/// of its public item IDs.
pub(super) async fn with_addresses(
    pool: &Pool,
    actor: &str,
    a: &Value,
    keys: &[&str],
) -> anyhow::Result<Value> {
    let mut a = a.clone();
    if a["workspace"].as_str().is_none_or(str::is_empty) {
        let ids: Vec<String> = keys
            .iter()
            .filter_map(|key| a[*key].as_str())
            .flat_map(|text| text.split(',').map(str::trim))
            .filter(|key| !key.is_empty() && !key.contains(':') && *key != "*")
            .map(str::to_owned)
            .collect();
        anyhow::ensure!(!ids.is_empty(), "missing workspace");
        a["workspace"] = json!(store::item_workspace(pool, actor, &ids).await?);
    }
    let workspace = a["workspace"]
        .as_str()
        .context("missing workspace")?
        .to_owned();
    let workspace = workspace.as_str();
    for key in keys {
        let Some(text) = a[*key].as_str() else {
            continue;
        };
        let parts: Vec<String> = text.split(',').map(|k| k.trim().to_owned()).collect();
        if parts
            .iter()
            .all(|k| k.is_empty() || k.contains(':') || k == "*")
        {
            continue;
        }
        let resolved = store::resolve_item_ids(pool, workspace, actor, &parts).await?;
        a[*key] = json!(resolved.join(","));
    }
    Ok(a)
}

fn number(a: &Value, key: &str) -> anyhow::Result<Option<u64>> {
    a.get(key)
        .map(|v| v.as_u64().with_context(|| format!("invalid {key}")))
        .transpose()
}

async fn objects(pool: &Pool, actor: &str, a: &Value) -> anyhow::Result<Value> {
    let workspace = a["workspace"].as_str().context("missing workspace")?;
    let fields = read_shapes::fields_argument(&a["fields"])?;
    let revision = number(a, "revision")?
        .map(u32::try_from)
        .transpose()
        .context("invalid object revision")?;
    let asked = number(a, "limit")?.map(|n| n as usize);
    let limit = asked.map_or(rc::PAGE, |n| n.min(rc::PAGE));
    // A list with fields takes up to 100 items; the 256 KB ceiling still holds.
    let list_limit = match fields {
        Some(_) => asked.map_or(rc::PAGE, |n| n.min(rc::FIELDS_PAGE)),
        None => limit,
    };
    anyhow::ensure!(a["links_of"].is_null(), joppa::removed_ops::LINKS_OF);
    if let Some(key) = a.get("in_relation").filter(|v| !v.is_null()) {
        let key = key.as_str().context("in_relation must be a string")?;
        return members(pool, workspace, actor, a, key, fields).await;
    }
    if let Some(address) = a["relations_of"].as_str() {
        let filter = |name: &str| match a.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_str()
                .map(Some)
                .with_context(|| format!("{name} must be a string")),
        };
        let reply = store::link_page(
            pool,
            workspace,
            actor,
            address,
            filter("direction")?,
            filter("relation")?,
            filter("kind")?,
            limit,
            a["cursor"].as_str(),
        )
        .await?;
        let reply = joppa::mcp_shapes::links(reply, a);
        return Ok(rc::strip_reply(reply));
    }
    anyhow::ensure!(a["work_filter"].is_null(), joppa::removed_ops::WORK_FILTER);
    // The node filters: parent with depth, kind, status, owner, assignee, ready and data.
    let filter = joppa::node_filter::NodeFilter::parse(a)?;
    let inbox =
        a["address"].is_null() && inbox_kind(pool, workspace, actor, filter.one_kind()).await?;
    let plain = a["address"].is_null()
        && fields.is_none()
        && ["related_to", "note_status"]
            .iter()
            .all(|k| a[*k].is_null())
        && !inbox;
    if plain {
        return store::compact_list(pool, workspace, actor, &filter, limit, a["cursor"].as_str())
            .await;
    }
    let history = a["history"].as_bool().unwrap_or(false);
    let scope = |address| store::ObjectScope {
        address,
        filter: filter.clone(),
        revision,
        history,
        limit: list_limit,
        // The cursor of a history read pages the revision headers here.
        cursor: a["cursor"].as_str().filter(|_| !history),
        note_status: a["note_status"].as_str(),
        related_to: a["related_to"].as_str(),
        data: fields.as_deref().is_some_and(rc::needs_data),
    };
    if let Some(addresses) = address::addresses(&a["address"])? {
        return address::read(
            pool,
            workspace,
            actor,
            a,
            &addresses,
            fields.as_deref(),
            scope,
        )
        .await;
    }
    let mut reply = store::read_objects(pool, workspace, actor, &scope(None)).await?;
    // A list in address order gives the bare address of its last item as the cursor,
    // as a plain list does, so one sweep may mix plain pages and pages with fields.
    if !inbox
        && let Some((p, address)) = reply["next_cursor"]
            .as_str()
            .and_then(|c| c.split_once(':'))
        && p.parse::<i64>().is_ok()
    {
        reply["next_cursor"] = json!(address);
    }
    if let Some(fields) = &fields {
        let items = reply["items"].as_array().into_iter().flatten();
        let data: Vec<&Value> = items.map(|item| &item["data"]).collect();
        check(fields, false, &data)?;
    }
    Ok(rc::list(reply, fields.as_deref()))
}

/// The member read of one relation (src/relation_members.rs), in pages of 25.
async fn members(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    a: &Value,
    key: &str,
    fields: Option<Vec<String>>,
) -> anyhow::Result<Value> {
    let sent = a.as_object().into_iter().flatten();
    let sent = sent.filter(|(_, v)| !v.is_null()).map(|(k, _)| k.as_str());
    joppa::relation_members::check_keys(sent)?;
    let limit = joppa::relation_members::page(number(a, "limit")?.map(|n| n as usize));
    let data = fields.as_deref().is_some_and(rc::needs_data);
    let cursor = a["cursor"].as_str();
    let reply = store::relation_members(pool, workspace, actor, key, limit, cursor, data).await?;
    if let Some(fields) = &fields {
        let items = reply["items"].as_array().into_iter().flatten();
        let data: Vec<&Value> = items.map(|item| &item["data"]).collect();
        check(fields, false, &data)?;
    }
    Ok(rc::list(reply, fields.as_deref()))
}

#[path = "protocol_reads_address.rs"]
mod address;

/// Refuse an older text field, and a detail key on a list read that no returned document holds.
/// A read by address (`detail`) takes the detail keys. Each other bare name reads its data field.
fn check(fields: &[String], detail: bool, data: &[&Value]) -> anyhow::Result<()> {
    rc::check_fields(fields, |head| {
        (detail && rc::DETAIL_FIELDS.contains(&head)) || data.iter().any(|d| d.get(head).is_some())
    })
}

/// An Inbox type lists unprocessed inputs first, so its list keeps the order of read_objects.
async fn inbox_kind(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    kind: Option<&str>,
) -> anyhow::Result<bool> {
    let Some(kind) = kind else {
        return Ok(false);
    };
    let tree = store::tree_schema(pool, workspace, actor, None).await?;
    Ok(tree["schema"]["types"][kind]["inbox"] == true)
}
