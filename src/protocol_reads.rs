//! The compact MCP reads of joppa_objects and joppa_tests (requirement:read-contract).
use anyhow::Context;
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, read_shapes, store};
use serde_json::{Value, json};

pub async fn compact(pool: &Pool, actor: &str, name: &str, a: &Value) -> anyhow::Result<Value> {
    if name == "joppa_objects" {
        objects(pool, actor, a).await
    } else {
        super::test_reads::tests(pool, actor, a).await
    }
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
    let limit = number(a, "limit")?.map_or(rc::PAGE, |n| (n as usize).min(rc::PAGE));
    // The data filter: a JSON object that document.data contains, as a JSONB `@>`.
    let contains = match &a["data"] {
        Value::Null => None,
        Value::Object(_) => Some(a["data"].to_string()),
        _ => anyhow::bail!("data filter must be an object"),
    };
    if let Some(address) = a["links_of"].as_str() {
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
    let inbox =
        a["address"].is_null() && inbox_kind(pool, workspace, actor, a["kind"].as_str()).await?;
    let plain = a["address"].is_null()
        && fields.is_none()
        && ["work_filter", "related_to", "note_status", "data"]
            .iter()
            .all(|k| a[*k].is_null())
        && !inbox;
    if plain {
        return store::compact_list(
            pool,
            workspace,
            actor,
            a["parent"].as_str(),
            a["kind"].as_str(),
            limit,
            a["cursor"].as_str(),
        )
        .await;
    }
    let history = a["history"].as_bool().unwrap_or(false);
    let scope = |address| store::ObjectScope {
        address,
        parent: a["parent"].as_str(),
        kind: a["kind"].as_str(),
        revision,
        history,
        limit,
        // The cursor of a history read pages the revision headers here.
        cursor: a["cursor"].as_str().filter(|_| !history),
        note_status: a["note_status"].as_str(),
        related_to: a["related_to"].as_str(),
        work_filter: a["work_filter"].as_str(),
        data: fields.as_deref().is_some_and(rc::needs_data),
        contains: contains.as_deref(),
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
        && a["work_filter"].is_null()
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
        check(pool, workspace, actor, fields, false, &data).await?;
    }
    Ok(rc::list(reply, fields.as_deref()))
}

#[path = "protocol_reads_address.rs"]
mod address;

/// Refuse a field that names no list field, no document field, no field of the tree schema
/// and no key of a returned document. A read by address also takes the detail keys.
async fn check(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    fields: &[String],
    detail: bool,
    data: &[&Value],
) -> anyhow::Result<()> {
    let found = |head: &str| {
        (detail && rc::DETAIL_FIELDS.contains(&head)) || data.iter().any(|d| d.get(head).is_some())
    };
    if rc::check_fields(fields, found).is_ok() {
        return Ok(());
    }
    let tree = store::tree_schema(pool, workspace, actor, None).await?;
    let types = tree["schema"]["types"].as_object().into_iter().flatten();
    let schema: Vec<&String> = types
        .flat_map(|(_, t)| ["references", "required_fields"].map(|k| &t[k]))
        .filter_map(Value::as_object)
        .flat_map(|m| m.keys())
        .collect();
    rc::check_fields(fields, |head| {
        found(head) || schema.iter().any(|k| *k == head)
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
