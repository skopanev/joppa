//! Part of `store_write`: the local ID that the write path generates for a new object.
//! The server generates again while the address is taken, at most 16 times in total.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The test hook: a comma list of 8-character local IDs. Each generation takes the next ID,
/// and after the list the random ID. Production never sets it.
const TEST_IDS: &str = "JOPPA_TEST_LOCAL_IDS";
/// The generations of one new object, the first included.
const ATTEMPTS: usize = 16;
/// The position in the test list: one for each server process, over all workspaces.
static POSITION: AtomicUsize = AtomicUsize::new(0);

/// Give a `save_object` without an id a free generated id. An explicit id stays as it is.
pub(crate) async fn generate_free(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    command: &mut Value,
) -> Result<()> {
    if command["op"] != "save_object" || command.get("id").is_some() {
        return generate(command);
    }
    let kind = command["kind"].as_str().unwrap_or_default().to_owned();
    let id = free_id(client, workspace, state, &kind, next_id).await?;
    command["id"] = json!(id);
    Ok(())
}

/// The first generated id whose address is free. `next` gives each generated id.
pub(crate) async fn free_id(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    kind: &str,
    mut next: impl FnMut() -> String,
) -> Result<String> {
    for _ in 0..ATTEMPTS {
        let id = next();
        if !taken(client, workspace, state, kind, &id).await? {
            return Ok(id);
        }
    }
    anyhow::bail!("no free local ID after {ATTEMPTS} attempts; retry the write")
}

/// An address is taken when the workspace has a current object of any state for it, a
/// historical alias, or a row of joppa.item_ids in any namespace. The state holds each
/// object that an earlier operation of the batch created.
pub(crate) async fn taken(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    kind: &str,
    id: &str,
) -> Result<bool> {
    let address = format!("{kind}:{id}");
    if state.objects.contains_key(&address) || state.object_aliases.contains_key(&address) {
        return Ok(true);
    }
    let row = client
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM joppa.item_ids WHERE workspace=$1 AND kind=$2 AND local_id=$3)",
            &[&workspace, &kind, &id],
        )
        .await?;
    Ok(row.get(0))
}

/// The next generated id: the next id of the test list, then the random id.
fn next_id() -> String {
    let Ok(list) = std::env::var(TEST_IDS) else {
        return short_id();
    };
    listed(&list, POSITION.fetch_add(1, Ordering::SeqCst)).unwrap_or_else(short_id)
}

/// The id at `position` of a comma list, if the list is that long.
fn listed(list: &str, position: usize) -> Option<String> {
    list.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .nth(position)
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "store_write_local_id_tests.rs"]
mod tests;
