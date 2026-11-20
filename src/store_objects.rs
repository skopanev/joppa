use super::*;
use crate::objects;

#[path = "objects_project.rs"]
mod project_rows;
pub(super) use project_rows::project;
#[path = "objects_tree.rs"]
mod tree;
pub use tree::tree_schema;
#[path = "objects_read.rs"]
mod object_read;
pub use object_read::{read_objects, relation_members};
#[path = "objects_cards.rs"]
mod cards;
pub use cards::{CardNeeds, read_cards};
#[path = "objects_compact_list.rs"]
mod compact_rows;
pub use compact_rows::compact_list;
#[path = "objects_counts.rs"]
mod counts;
pub use counts::count_objects;
#[path = "objects_link_groups.rs"]
mod link_groups;
pub use link_groups::link_page;
#[path = "objects_list.rs"]
mod object_list;
#[path = "objects_list_rows.rs"]
mod object_list_rows;
#[path = "objects_tones.rs"]
mod tone_items;
use tone_items::add_status_tones;
#[path = "objects_computed.rs"]
mod computed;
#[path = "objects_projection_delta.rs"]
mod delta;
#[path = "objects_dimensions.rs"]
mod dimensions;
#[path = "objects_projection.rs"]
mod projection;
pub(super) use delta::update as update_status_projection;
use projection::apply_rollups;
pub(super) use projection::{
    STATUS_PROJECTION_VERSION, rebuild as rebuild_status_projection, stored_statuses,
};

async fn authorized(
    pool: &Pool,
    workspace: &str,
    actor: &str,
) -> Result<(deadpool_postgres::Object, String)> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    Ok((conn, workspace))
}

/// The address of each key: a key with `:` and `*` stay, any other key is a public item ID
/// such as `joppa-R-DP6KXM62`. An unknown public item ID is refused.
pub async fn resolve_item_ids(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    keys: &[String],
) -> Result<Vec<String>> {
    let resolved = resolve_or_keep(pool, workspace, actor, keys).await?;
    for (key, address) in keys.iter().zip(&resolved) {
        ensure!(key != address || is_address(key), "unknown object");
    }
    Ok(resolved)
}

fn is_address(key: &str) -> bool {
    key.is_empty() || key.contains(':') || key == "*"
}

/// `resolve_item_ids`, but an unknown public item ID stays as it is, so a read of many
/// addresses names it in `missing`.
pub async fn resolve_or_keep(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    keys: &[String],
) -> Result<Vec<String>> {
    let address = |key: &String| is_address(key);
    if keys.iter().all(address) {
        return Ok(keys.to_vec());
    }
    let (conn, workspace) = authorized(pool, workspace, actor).await?;
    // One query resolves every public item ID of the request, in the order of the request.
    let ids: Vec<&str> = keys
        .iter()
        .filter(|k| !address(k))
        .map(String::as_str)
        .collect();
    let rows = conn
        .query(
            "SELECT (SELECT kind||':'||local_id FROM joppa.item_ids WHERE workspace=$1 AND id=joppa.short_item_id(k.id) LIMIT 1) FROM unnest($2::text[]) WITH ORDINALITY k(id,n) ORDER BY k.n",
            &[&workspace, &ids],
        )
        .await?;
    let mut found = rows.iter().map(|r| r.get::<_, Option<String>>(0));
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        out.push(match address(key) {
            true => key.clone(),
            false => found.next().flatten().unwrap_or_else(|| key.clone()),
        });
    }
    Ok(out)
}

/// Whether `kind` is an Inbox type, from the types of the board projection. It reads no tree
/// schema revision (ac:1z012wub).
pub async fn inbox_type(pool: &Pool, workspace: &str, actor: &str, kind: &str) -> Result<bool> {
    let (conn, workspace) = authorized(pool, workspace, actor).await?;
    let sql = "SELECT (first_view->'object_types'->$2->>'inbox')::boolean FROM joppa.board_cache WHERE workspace=$1";
    let row = conn.query_opt(sql, &[&workspace, &kind]).await?;
    Ok(row
        .and_then(|r| r.get::<_, Option<bool>>(0))
        .unwrap_or(false))
}

/// The workspace of public item IDs, for a read that names no workspace. Every ID must
/// belong to one workspace that the actor can read; any other case reads as an unknown object.
pub async fn item_workspace(pool: &Pool, actor: &str, ids: &[String]) -> Result<String> {
    let conn = pool.get().await?;
    let rows = conn
        .query(
            "SELECT DISTINCT i.workspace FROM unnest($1::text[]) k(id)
             JOIN joppa.item_ids i ON i.id=joppa.short_item_id(k.id)",
            &[&ids],
        )
        .await?;
    // An unknown ID beside known IDs stays for the read, which names it in missing. With no
    // known ID the call names the first ID.
    let found: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    let first = ids.first().map_or("", String::as_str);
    ensure!(!found.is_empty(), "unknown object: {first}");
    // An ID of a workspace that the caller cannot read reads as unknown.
    for workspace in &found {
        access::check(&**conn, workspace, actor, false)
            .await
            .map_err(|_| anyhow::anyhow!("unknown object: {first}"))?;
    }
    ensure!(
        found.len() == 1,
        "the public item IDs of one call belong to one workspace"
    );
    Ok(found[0].clone())
}

#[derive(Default)]
pub struct ObjectScope<'a> {
    pub address: Option<&'a str>,
    /// The node filters of a list: parent with depth, kind, status, owner, assignee and data.
    pub filter: crate::node_filter::NodeFilter,
    pub revision: Option<u32>,
    pub history: bool,
    pub limit: usize,
    pub cursor: Option<&'a str>,
    pub note_status: Option<&'a str>,
    pub related_to: Option<&'a str>,
    /// Each list item also gives the full `document.data`.
    pub data: bool,
}
