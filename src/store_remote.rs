//! Reads and checks across workspaces: remote links and child workspace nodes.
//!
//! The state of another workspace never enters this journal. A write checks the targets
//! once, and each read or claim reads their current state with the access of its actor.
use super::*;
use crate::remote_links::RemoteLink;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The advisory lock key of a workspace. Every write and record of it takes this lock.
const WORKSPACE_LOCK: &str = "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))";
/// One global lock for writes of blocking remote links, so two writes in different
/// workspaces cannot close one cycle together.
const REMOTE_CYCLE_LOCK: i64 = 91827103;
/// A cycle walk stops after this many nodes.
const WALK_LIMIT: usize = 10_000;

/// Replace each workspace name in a command with the immutable workspace ID, before the
/// journal stores the command.
pub(super) async fn canonical_ids(client: &impl GenericClient, value: &mut Value) -> Result<()> {
    if value["op"] == "set_remote_links" {
        for link in value["links"].as_array_mut().into_iter().flatten() {
            if let Some(name) = link["workspace"].as_str().map(str::to_owned) {
                link["workspace"] = json!(resolve_workspace(client, &name).await?);
            }
        }
    }
    if value["op"] == "save_object"
        && let Some(name) = value["data"]["workspace"].as_str().map(str::to_owned)
    {
        value["data"]["workspace"] = json!(resolve_workspace(client, &name).await?);
    }
    Ok(())
}

/// Check that the actor can read another workspace, named by its immutable ID.
async fn readable(client: &impl GenericClient, own: &str, actor: &str, id: &str) -> Result<()> {
    ensure!(
        id != own,
        "a remote link or a child workspace names another workspace"
    );
    access::check(client, id, actor, false)
        .await
        .map_err(|_| anyhow::anyhow!("no read access to workspace {id}"))
}

/// The present target with its current revision, through an object alias.
pub(super) async fn target(
    client: &impl GenericClient,
    link: &RemoteLink,
) -> Result<Option<(String, i32)>> {
    Ok(client
        .query_opt(
            "SELECT o.address,o.revision FROM joppa.objects o WHERE o.workspace=$1 AND o.present AND o.address=COALESCE((SELECT a.target FROM joppa.object_aliases a WHERE a.workspace=$1 AND a.address=$2),$2)",
            &[&link.workspace, &link.target],
        )
        .await?
        .map(|r| (r.get(0), r.get(1))))
}

/// Check the links of a set_remote_links command before the write applies it.
pub(super) async fn check_links(
    client: &impl GenericClient,
    own: &str,
    actor: &str,
    source: &str,
    links: &[RemoteLink],
) -> Result<()> {
    for link in links {
        readable(client, own, actor, &link.workspace).await?;
        let (_, current) = target(client, link).await?.with_context(|| {
            format!(
                "unknown or archived object {} in workspace {}",
                link.target, link.workspace
            )
        })?;
        ensure!(
            link.revision
                .is_none_or(|r| i64::from(r) <= i64::from(current)),
            "unknown linked revision"
        );
    }
    if links.iter().any(|l| l.blocking) {
        client
            .execute("SELECT pg_advisory_xact_lock($1)", &[&REMOTE_CYCLE_LOCK])
            .await?;
        for link in links.iter().filter(|l| l.blocking) {
            let start = (
                link.workspace.clone(),
                target(client, link).await?.unwrap().0,
            );
            ensure!(
                !reaches(client, start, (own.to_owned(), source.to_owned())).await?,
                "dependency cycle across workspaces"
            );
        }
    }
    Ok(())
}

/// Whether a walk from `start` over parents and blocking links, local and remote, reaches
/// `goal`. The walk is the same as the local cycle check, across workspaces.
async fn reaches(
    client: &impl GenericClient,
    start: (String, String),
    goal: (String, String),
) -> Result<bool> {
    let mut queue = VecDeque::from([start]);
    let mut seen = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if node == goal {
            return Ok(true);
        }
        if !seen.insert(node.clone()) {
            continue;
        }
        ensure!(seen.len() <= WALK_LIMIT, "the dependency walk is too long");
        let (ws, address) = &node;
        for row in client.query(
            "SELECT o.parent FROM joppa.objects o WHERE o.workspace=$1 AND o.address=$2 AND o.parent IS NOT NULL
             UNION ALL SELECT COALESCE(a.target,l.target) FROM joppa.object_links l LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target WHERE l.workspace=$1 AND l.source=$2 AND l.blocking AND l.relation='depends_on'",
            &[ws, address],
        ).await? {
            queue.push_back((ws.clone(), row.get(0)));
        }
        for link in stored_links(client, ws, address).await?.links {
            if link.blocking {
                let next = target(client, &link)
                    .await?
                    .map_or(link.target.clone(), |t| t.0);
                queue.push_back((link.workspace, next));
            }
        }
    }
    Ok(false)
}

/// The stored remote links of one object with their revision.
pub(super) async fn stored_links(
    client: &impl GenericClient,
    workspace: &str,
    address: &str,
) -> Result<crate::remote_links::RemoteLinks> {
    Ok(match client
        .query_opt(
            "SELECT value::text FROM joppa.state_entries WHERE workspace=$1 AND section='remote_links' AND key=$2",
            &[&workspace, &address],
        )
        .await?
    {
        Some(row) => serde_json::from_str(row.get(0))?,
        None => Default::default(),
    })
}

/// The workspaces that blocking remote links of this workspace name, from its stored state.
pub(super) async fn blocking_workspaces(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<BTreeSet<String>> {
    Ok(client
        .query(
            "SELECT DISTINCT l->>'workspace' FROM joppa.state_entries e CROSS JOIN LATERAL jsonb_array_elements(e.value->'links') l WHERE e.workspace=$1 AND e.section='remote_links' AND (l->>'blocking')::boolean",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// Lock this workspace and the given ones in sorted ID order, so no two writes wait for
/// each other.
pub(super) async fn lock_sorted(
    client: &impl GenericClient,
    own: &str,
    others: &BTreeSet<String>,
) -> Result<()> {
    let mut all = others.clone();
    all.insert(own.to_owned());
    for id in &all {
        client.execute(WORKSPACE_LOCK, &[id]).await?;
    }
    Ok(())
}

/// Check the child workspace of a saved object.
pub(super) async fn check_child(
    client: &impl GenericClient,
    own: &str,
    actor: &str,
    child: &str,
) -> Result<()> {
    readable(client, own, actor, child).await
}

/// The remote nodes of a live claim graph: each blocking link of a present source.
pub(super) fn remote_edges(state: &Workspace) -> BTreeMap<String, Vec<String>> {
    state
        .remote_links
        .iter()
        .filter(|(source, _)| {
            state
                .objects
                .get(*source)
                .is_some_and(|o| o.current().present)
        })
        .map(|(source, links)| {
            (
                source.clone(),
                links
                    .links
                    .iter()
                    .filter(|l| l.blocking)
                    .map(RemoteLink::node)
                    .collect(),
            )
        })
        .collect()
}
