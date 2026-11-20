//! The detail keys of a list read: comments, comments_count, files, files_count and
//! references. One batch read of the page gives them, only when the fields name them.
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, store};
use serde_json::Value;
use std::collections::BTreeMap;

/// Set the named detail keys on each item of the compact list `out`.
pub(super) async fn add(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    out: &mut Value,
    fields: &[String],
) -> anyhow::Result<()> {
    let wanted: Vec<String> = fields
        .iter()
        .filter(|f| rc::LIST_DETAIL_FIELDS.contains(&rc::head(f)))
        .cloned()
        .collect();
    let items = out["items"].as_array().into_iter().flatten();
    let addresses: Vec<String> = items
        .filter_map(|i| i["address"].as_str().map(str::to_owned))
        .collect();
    if wanted.is_empty() || addresses.is_empty() {
        return Ok(());
    }
    let asked: Vec<&str> = addresses.iter().map(String::as_str).collect();
    let needs = rc::card_needs(Some(&wanted));
    let (found, _) = store::read_cards(pool, workspace, actor, &asked, needs).await?;
    let mut details = BTreeMap::new();
    for (address, reply) in found {
        let detail = rc::detail_keys(reply, &address, None)?;
        details.insert(address, detail);
    }
    for item in out["items"].as_array_mut().into_iter().flatten() {
        let Some(detail) = item["address"].as_str().and_then(|a| details.get(a)) else {
            continue;
        };
        for field in &wanted {
            item[field.as_str()] = rc::detail_value(detail, field);
        }
    }
    Ok(())
}
