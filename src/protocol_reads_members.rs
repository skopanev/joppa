//! The member read of joppa_objects: the nodes of one relation key (src/relation_members.rs).
use super::{check, number};
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, store};
use serde_json::Value;

/// The member read of one relation (src/relation_members.rs), in pages of 25.
pub(super) async fn read(
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
    let needs = rc::list_needs(fields.as_deref());
    let cursor = a["cursor"].as_str();
    // Each relations.<key> of fields names a relation of the registry (ac:ig3uv35d).
    let keys = joppa::relation_members::relation_keys(fields.as_deref().unwrap_or_default());
    let page = (limit, cursor, data, needs, keys.as_slice());
    let reply = store::relation_members_with(pool, workspace, actor, key, page).await?;
    if let Some(fields) = &fields {
        let items = reply["items"].as_array().into_iter().flatten();
        let data: Vec<&Value> = items.map(|item| &item["data"]).collect();
        check(fields, false, &data)?;
    }
    let mut reply = rc::list(reply, fields.as_deref());
    if let Some(items) = reply["items"].as_array_mut() {
        items.iter_mut().for_each(joppa::relation_members::nest);
    }
    Ok(reply)
}
