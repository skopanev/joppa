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
pub use object_read::read_objects;
#[path = "objects_compact_list.rs"]
mod compact_rows;
pub use compact_rows::compact_list;
#[path = "objects_link_groups.rs"]
mod link_groups;
pub use link_groups::link_page;
#[path = "objects_list.rs"]
mod object_list;
#[path = "objects_list_rows.rs"]
mod object_list_rows;
#[path = "objects_wait_items.rs"]
mod wait_items;
use wait_items::add_waits;
#[path = "objects_tones.rs"]
mod tone_items;
use tone_items::add_status_tones;
#[path = "objects_projection.rs"]
mod projection;
#[path = "objects_ready_rollup.rs"]
mod ready_rollup;
#[path = "objects_projection_delta.rs"]
mod delta;
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
/// such as `joppa-R-DP6KXM62`.
pub async fn resolve_item_ids(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    keys: &[String],
) -> Result<Vec<String>> {
    let address = |key: &String| key.is_empty() || key.contains(':') || key == "*";
    if keys.iter().all(address) {
        return Ok(keys.to_vec());
    }
    let (conn, workspace) = authorized(pool, workspace, actor).await?;
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        if address(key) {
            out.push(key.clone());
            continue;
        }
        let row = conn
            .query_opt(
                "SELECT kind||':'||local_id FROM joppa.item_ids WHERE workspace=$1 AND id=joppa.short_item_id($2)",
                &[&workspace, key],
            )
            .await?;
        out.push(row.map(|r| r.get(0)).context("unknown object")?);
    }
    Ok(out)
}

/// The workspace of public item IDs, for a read that names no workspace. Every ID must
/// belong to one workspace that the actor can read; any other case reads as an unknown object.
pub async fn item_workspace(pool: &Pool, actor: &str, ids: &[String]) -> Result<String> {
    let conn = pool.get().await?;
    let rows = conn
        .query(
            "SELECT DISTINCT i.workspace FROM unnest($1::text[]) k(id)
             LEFT JOIN joppa.item_ids i ON i.id=joppa.short_item_id(k.id)",
            &[&ids],
        )
        .await?;
    let found: Vec<Option<String>> = rows.iter().map(|r| r.get(0)).collect();
    // An unknown ID reads as unknown, also beside known IDs.
    let found: Vec<String> = found
        .into_iter()
        .collect::<Option<_>>()
        .context("unknown object")?;
    // An ID of a workspace that the caller cannot read reads as unknown.
    for workspace in &found {
        access::check(&**conn, workspace, actor, false)
            .await
            .map_err(|_| anyhow::anyhow!("unknown object"))?;
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
    /// The node filters of a list: parent with depth, kind, status, owner, assignee, ready
    /// and data.
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
