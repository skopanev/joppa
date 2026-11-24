//! The search rows that writes leave for later (migrations 072 and 073). A write adds one row
//! for each object that it reaches, with its journal position. Each search read builds their rows first,
//! and the background pass of the indexer builds them after the writes of a workspace pause, so
//! a write builds no search row. The builders hold the search lock, not the write lock: a
//! builder removes only the objects of the position of its state, and a later write keeps its
//! rows for the next build.
use super::*;
use std::collections::BTreeSet;

/// The address that names every object of the workspace.
const ALL: &str = "*";

/// The address of a write that reaches no object: the build moves only the search position.
const MOVE: &str = "#";

/// The lock of the search builders of one workspace. Writes take another key.
pub(super) const LOCK: &str = "SELECT pg_advisory_xact_lock(hashtextextended($1,91827103))";

/// The pause after a write before the background pass builds the rows.
const QUIET: std::time::Duration = std::time::Duration::from_millis(300);

/// Add the objects that the write from `base` to `position` reaches: `selected`, or every
/// object. Search rows that were current at `base` stay current for the other objects. Rows
/// that lagged without a pending object need every object.
pub(super) async fn add(
    client: &impl GenericClient,
    workspace: &str,
    selected: Option<&BTreeSet<String>>,
    (base, position): (u64, u64),
) -> Result<()> {
    let sql = "SELECT (SELECT position FROM joppa.search_state WHERE workspace=$1), EXISTS(SELECT 1 FROM joppa.search_pending WHERE workspace=$1)";
    let row = client.query_one(sql, &[&workspace]).await?;
    let current = row.get::<_, bool>(1) || row.get::<_, Option<i64>>(0) == Some(base as i64);
    let list: Vec<&str> = match selected {
        Some(selected) if current && selected.is_empty() => vec![MOVE],
        Some(selected) if current => selected.iter().map(String::as_str).collect(),
        _ => vec![ALL],
    };
    // Only an insert: the rows of a write never meet the rows that a build removes.
    let sql = "INSERT INTO joppa.search_pending(workspace,address,position) SELECT $1,unnest($2::text[]),$3";
    client
        .execute(sql, &[&workspace, &list, &(position as i64)])
        .await?;
    Ok(())
}

/// Build the search rows of the pending objects of `workspace` up to the position of its
/// current state, and remove them. False when no such object waits. The caller holds LOCK.
pub(super) async fn build(client: &impl GenericClient, workspace: &str) -> Result<bool> {
    let board = client
        .query_opt(
            "SELECT position, COALESCE(state->>'workspace_id',$1), COALESCE(state->>'reference_prefix',$1) FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    let sql =
        "DELETE FROM joppa.search_pending WHERE workspace=$1 AND position<=$2 RETURNING address";
    let mut addresses = BTreeSet::new();
    let mut had_move = false;
    if let Some(board) = board {
        let (pos, canonical, prefix): (i64, String, String) =
            (board.get(0), board.get(1), board.get(2));
        let rows = client.query(sql, &[&workspace, &pos]).await?;
        if rows.is_empty() {
            return Ok(false);
        }
        addresses.extend(rows.iter().map(|r| r.get::<_, String>(0)));
        had_move = addresses.remove(MOVE);
        if !addresses.contains(ALL) && addresses.is_empty() {
            let moved = client
                .execute(
                    "UPDATE joppa.search_state SET position=GREATEST(position,$2),canonical_id=$3,reference_prefix=$4 WHERE workspace=$1",
                    &[&workspace, &pos, &canonical, &prefix],
                )
                .await?;
            if moved > 0 {
                return Ok(true);
            }
            addresses.insert(ALL.into());
        }
    }
    let (state, position) = super::state_cache::read(client, workspace).await?;
    let rows = client.query(sql, &[&workspace, &(position as i64)]).await?;
    if !had_move && addresses.is_empty() && rows.is_empty() {
        return Ok(false);
    }
    for r in rows {
        let address: String = r.get(0);
        if address != MOVE {
            addresses.insert(address);
        }
    }
    let all = addresses.remove(ALL);
    let selected = (!all).then_some(&addresses);
    super::search::project(client, workspace, &state, position, selected).await?;
    super::search_revisions::project(client, workspace, selected).await?;
    // With every object, the revision rows of the named objects renew too, such as after a
    // redaction.
    if all && !addresses.is_empty() {
        super::search_revisions::project(client, workspace, Some(&addresses)).await?;
    }
    Ok(true)
}

/// The background pass: the pending objects of each workspace.
pub async fn pass(pool: &Pool) -> Result<()> {
    let sql = "SELECT DISTINCT workspace FROM joppa.search_pending";
    let ids: Vec<String> = pool
        .get()
        .await?
        .query(sql, &[])
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for id in ids {
        super::search::current(pool, &id, None).await?;
    }
    super::stream::trim_heap();
    Ok(())
}

/// Wait until no write wakes the indexer for QUIET. A wake asks for no reconcile pass, so
/// the answer is always false.
pub async fn quiet(notify: &tokio::sync::Notify) -> bool {
    while tokio::time::timeout(QUIET, notify.notified()).await.is_ok() {}
    false
}
