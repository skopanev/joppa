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
#[path = "objects_list.rs"]
mod object_list;
#[path = "objects_list_rows.rs"]
mod object_list_rows;
#[path = "objects_wait_items.rs"]
mod wait_items;
use wait_items::{add_priorities, add_waits};
#[path = "objects_tones.rs"]
mod tone_items;
use tone_items::add_status_tones;
#[path = "objects_summaries.rs"]
mod summary_items;
use summary_items::add_verification_summaries;
#[path = "objects_projection.rs"]
mod projection;
pub use projection::ProjectionNotReady;
use projection::apply_rollups;
pub(super) use projection::projected_waits;
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

#[derive(Default)]
pub struct ObjectScope<'a> {
    pub address: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub revision: Option<u32>,
    pub history: bool,
    pub limit: usize,
    pub cursor: Option<&'a str>,
    pub note_status: Option<&'a str>,
    pub related_to: Option<&'a str>,
    pub work_filter: Option<&'a str>,
}

/// The priority values of the current tree schema, the first value first in work order.
pub(super) async fn priority_values(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<Vec<String>> {
    let saved: Option<String> = client
        .query_opt(
            "SELECT (value->'tree_schema'->-1->'priorities')::text FROM joppa.state_entries WHERE workspace=$1 AND section='meta' AND key=''",
            &[&workspace],
        )
        .await?
        .and_then(|row| row.get(0));
    Ok(saved
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_else(|| ["p0", "p1", "p2"].map(String::from).to_vec()))
}

/// The SQL rank array of the priority chain of `node` (a column of joppa.objects or of the
/// Task queue): the rank of each priority from the root down to it. `values` is the list.
pub(super) fn priority_chain_sql(workspace: &str, node: &str, values: &str) -> String {
    format!(
        "(WITH RECURSIVE up(address,depth) AS (
            SELECT {node},0
            UNION ALL
            SELECT COALESCE(al.target,p.parent),up.depth+1 FROM up
            JOIN joppa.objects p ON p.workspace={workspace} AND p.address=up.address
            LEFT JOIN joppa.object_aliases al ON al.workspace=p.workspace AND al.address=p.parent
            WHERE p.parent IS NOT NULL AND up.depth<64)
        SELECT array_agg(COALESCE(array_position({values},pe.value#>>'{{}}'),cardinality({values})) ORDER BY up.depth DESC)
        FROM up LEFT JOIN joppa.state_entries pe ON pe.workspace={workspace} AND pe.section='priorities' AND pe.key=up.address)"
    )
}
