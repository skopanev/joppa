//! The current state of each workspace after the last write of this process. A write that
//! finds the same key in the database starts from this state and reads no current rows.
//!
//! The key is the position and the row version (`xmin`) of the board row. Each write and each
//! projection rewrites that row, so a change by another process or another path gives a new
//! key. The state goes into memory only after the commit, and a write takes it out first, so
//! a failed write or a deleted workspace leaves no state. The kept state is the state that a
//! read of the rows gives: only the last revisions, and compact inbox documents.
use super::*;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// The position and the row version of the board row.
type Key = (i64, String);

static STATES: LazyLock<Mutex<HashMap<String, (Key, Workspace)>>> = LazyLock::new(Default::default);

async fn key(client: &impl GenericClient, workspace: &str) -> Result<Option<Key>> {
    let sql = "SELECT position,xmin::text FROM joppa.board_cache WHERE workspace=$1";
    let row = client.query_opt(sql, &[&workspace]).await?;
    Ok(row.map(|row| (row.get(0), row.get(1))))
}

/// The current state of `workspace` and its position. The caller holds the advisory lock of
/// the workspace.
pub(super) async fn take(client: &impl GenericClient, workspace: &str) -> Result<(Workspace, u64)> {
    let kept = STATES.lock().unwrap().remove(workspace);
    if let Some((kept_key, state)) = kept
        && let Some(key) = key(client, workspace).await?
        && kept_key == key
    {
        crate::probe::count(&crate::probe::STATE_REUSES);
        #[cfg(debug_assertions)]
        same_as_rows(client, workspace, &state).await?;
        return Ok((state, key.0 as u64));
    }
    state_rows::current(client, workspace).await
}

/// The state of a write, to keep after its transaction commits.
pub(super) struct Kept {
    workspace: String,
    key: Key,
    state: Workspace,
}

impl Kept {
    /// Keep the state. Call it only after the commit succeeded.
    pub(super) fn keep(self) {
        let mut states = STATES.lock().unwrap();
        states.insert(self.workspace, (self.key, self.state));
    }
}

/// Read the key that the write gives, and make the state equal to a read of its rows. Call it
/// after the last change of the transaction.
pub(super) async fn settle(
    client: &impl GenericClient,
    workspace: &str,
    mut state: Workspace,
) -> Result<Option<Kept>> {
    let Some(key) = key(client, workspace).await? else {
        return Ok(None);
    };
    let types = crate::objects::schema(&state).types;
    for (address, object) in &mut state.objects {
        object
            .revisions
            .drain(..object.revisions.len().saturating_sub(1));
        let compact = types.get(&object.kind).is_some_and(|t| {
            t.inbox == Some(true) && ["group", "document"].contains(&t.role.as_str())
        });
        // A compact row has no body and no description in the data of its revision.
        if compact && state.unloaded_objects.insert(address.clone()) {
            let data = &mut object.revisions[0].data;
            if let Some(data) = data.as_object_mut() {
                data.remove("body");
                data.remove("description");
            }
        }
    }
    for requirement in state.requirements.values_mut() {
        requirement
            .revisions
            .drain(..requirement.revisions.len().saturating_sub(1));
    }
    state
        .tree_schema
        .drain(..state.tree_schema.len().saturating_sub(1));
    // A removed field of an older schema is never stored: the round trip drops it, as a read
    // of the meta row does.
    state.tree_schema = serde_json::from_value(serde_json::to_value(&state.tree_schema)?)?;
    (state.alias, state.claim_time_ms) = (None, None);
    (state.in_place, state.replaying) = (false, false);
    let workspace = workspace.to_owned();
    Ok(Some(Kept {
        workspace,
        key,
        state,
    }))
}

/// In a debug build, each reuse proves that the kept state is the state of the rows.
#[cfg(debug_assertions)]
async fn same_as_rows(
    client: &impl GenericClient,
    workspace: &str,
    kept: &Workspace,
) -> Result<()> {
    let (rows, _) = state_rows::current(client, workspace).await?;
    // This proof is not a load of the write: the counter stays as it was.
    crate::probe::STATE_LOADS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    ensure!(
        serde_json::to_value(kept)? == serde_json::to_value(&rows)?
            && kept.unloaded_objects == rows.unloaded_objects,
        "the kept state of {workspace} differs from its rows"
    );
    Ok(())
}
