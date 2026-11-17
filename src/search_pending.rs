//! The search rows that writes leave for later (migration 072). A write adds the objects that
//! it reaches. Each search read builds their rows first, and the background pass of the
//! indexer builds them after the writes of a workspace pause, so a write builds no search row.
use super::*;
use std::collections::BTreeSet;

/// The address that names every object of the workspace.
const ALL: &str = "*";

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
        Some(selected) if current => selected.iter().map(String::as_str).collect(),
        _ => vec![ALL],
    };
    // A write that reaches no object moves current rows to its position.
    if list.is_empty() {
        let sql = "UPDATE joppa.search_state SET position=$3 WHERE workspace=$1 AND position=$2";
        let (base, position) = (base as i64, position as i64);
        client.execute(sql, &[&workspace, &base, &position]).await?;
        return Ok(());
    }
    let sql = "INSERT INTO joppa.search_pending(workspace,address) SELECT $1,unnest($2::text[]) ON CONFLICT DO NOTHING";
    client.execute(sql, &[&workspace, &list]).await?;
    Ok(())
}

/// Build the search rows of the pending objects of `workspace` and remove them. False when no
/// object waits. The caller holds the write lock of the workspace.
pub(super) async fn build(client: &impl GenericClient, workspace: &str) -> Result<bool> {
    let sql = "DELETE FROM joppa.search_pending WHERE workspace=$1 RETURNING address";
    let rows = client.query(sql, &[&workspace]).await?;
    if rows.is_empty() {
        return Ok(false);
    }
    let addresses: BTreeSet<String> = rows.iter().map(|r| r.get(0)).collect();
    let selected = (!addresses.contains(ALL)).then_some(addresses);
    let (state, position) = super::state_cache::read(client, workspace).await?;
    super::search::project(client, workspace, &state, position, selected.as_ref()).await?;
    super::search_revisions::project(client, workspace, selected.as_ref()).await?;
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
    Ok(())
}

/// Wait until no write wakes the indexer for QUIET. A wake asks for no reconcile pass, so
/// the answer is always false.
pub async fn quiet(notify: &tokio::sync::Notify) -> bool {
    while tokio::time::timeout(QUIET, notify.notified()).await.is_ok() {}
    false
}
