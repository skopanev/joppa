//! The current state of remote links and child workspaces, read with the access of an
//! actor. Nothing here is stored: each read and each claim computes it again.
use super::*;
use crate::remote_links::RemoteLink;
use std::collections::{BTreeSet, VecDeque};

/// A readiness walk stops after this many targets and then counts as not ready.
const READY_LIMIT: usize = 1_000;

/// Green, red and total counts of the present criteria at and below an address, or of the
/// whole workspace without one, and whether a node there waits for a local dependency.
async fn criteria(
    client: &impl GenericClient,
    workspace: &str,
    root: Option<&str>,
) -> Result<(i64, i64, i64, bool)> {
    let row = client.query_one(
        "WITH RECURSIVE sub(address) AS (SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND o.present AND ($2::text IS NULL AND o.parent IS NULL OR o.address=$2)
           UNION SELECT o.address FROM joppa.objects o JOIN sub ON o.parent=sub.address WHERE o.workspace=$1 AND o.present)
         SELECT count(*) FILTER (WHERE r.ac AND o.test_status->>'status'='green'),count(*) FILTER (WHERE r.ac AND o.test_status->>'status'='red'),count(*) FILTER (WHERE r.ac),
                bool_or(jsonb_typeof(o.rollup->'waits_for')='object')
         FROM joppa.objects o JOIN sub ON sub.address=o.address JOIN joppa.board_cache b ON b.workspace=o.workspace
         CROSS JOIN LATERAL (SELECT b.first_view->'object_types'->o.kind->>'role'='ac' AS ac) r
         WHERE o.workspace=$1",
        &[&workspace, &root],
    ).await?;
    Ok((
        row.get(0),
        row.get(1),
        row.get(2),
        row.get::<_, Option<bool>>(3).unwrap_or(false),
    ))
}

/// The remote links that apply to an address: its own, those of the nodes below it, and
/// those of its ancestors, which the nodes below it inherit.
async fn applying_links(
    client: &impl GenericClient,
    workspace: &str,
    address: &str,
) -> Result<Vec<RemoteLink>> {
    let mut links = vec![];
    for row in client.query(
        "WITH RECURSIVE down(a) AS (SELECT $2::text UNION SELECT o.address FROM joppa.objects o JOIN down ON o.parent=down.a WHERE o.workspace=$1 AND o.present),
           up(a) AS (SELECT o.parent FROM joppa.objects o WHERE o.workspace=$1 AND o.address=$2 AND o.parent IS NOT NULL
             UNION SELECT o.parent FROM joppa.objects o JOIN up ON o.address=up.a WHERE o.workspace=$1 AND o.parent IS NOT NULL)
         SELECT e.value::text FROM joppa.state_entries e WHERE e.workspace=$1 AND e.section='remote_links' AND (e.key IN (SELECT a FROM down) OR e.key IN (SELECT a FROM up))",
        &[&workspace, &address],
    ).await? {
        let stored: crate::remote_links::RemoteLinks = serde_json::from_str(row.get(0))?;
        links.extend(stored.links);
    }
    Ok(links)
}

/// The link as the actor sees it now: access, presence and the pin against the target.
/// Each key is always present, null when the actor cannot know it.
async fn link_state(
    client: &impl GenericClient,
    actor: &str,
    link: &RemoteLink,
) -> Result<(Value, Option<String>)> {
    if access::check(client, &link.workspace, actor, false)
        .await
        .is_err()
    {
        return Ok((
            json!({"access":false,"present":null,"current_revision":null,"review_required":null,"satisfied":false}),
            None,
        ));
    }
    let Some((address, current, contract)) = super::remote::target_contract(client, link).await?
    else {
        // An archived or absent target asks for review; it never satisfies a dependency.
        return Ok((
            json!({"access":true,"present":false,"current_revision":null,"review_required":true,"satisfied":false}),
            None,
        ));
    };
    // A pin stays current from the contract revision to the current revision.
    let stale = link
        .revision
        .is_some_and(|r| i64::from(contract) > i64::from(r));
    Ok((
        json!({"access":true,"present":true,"current_revision":current,"contract_revision":contract,"review_required":stale,"satisfied":false}),
        (!stale).then_some(address),
    ))
}

/// Whether a blocking target is ready for the actor: every criterion at and below it is
/// green, no node there waits for a local dependency, and each remote dependency that
/// applies there is readable, present, at its pinned revision and ready too.
/// A write passes the workspaces that it locked; a read passes None. A write that reaches
/// a workspace outside its locks fails closed and asks for a retry.
async fn target_ready(
    client: &impl GenericClient,
    actor: &str,
    workspace: &str,
    address: &str,
    locked: Option<&BTreeSet<String>>,
) -> Result<bool> {
    let mut queue = VecDeque::from([(workspace.to_owned(), address.to_owned())]);
    let mut seen = BTreeSet::new();
    while let Some((ws, address)) = queue.pop_front() {
        if !seen.insert((ws.clone(), address.clone())) {
            continue;
        }
        if seen.len() > READY_LIMIT {
            return Ok(false);
        }
        let (green, _, total, waits) = criteria(client, &ws, Some(&address)).await?;
        if total == 0 || green != total || waits {
            return Ok(false);
        }
        for link in applying_links(client, &ws, &address)
            .await?
            .into_iter()
            .filter(|l| l.blocking)
        {
            ensure!(
                locked.is_none_or(|l| l.contains(&link.workspace)),
                "the remote links changed during this write; send it again"
            );
            match link_state(client, actor, &link).await? {
                (_, Some(next)) => queue.push_back((link.workspace, next)),
                (_, None) => return Ok(false),
            }
        }
    }
    Ok(true)
}

/// The full status of one remote link for this actor.
pub(crate) async fn link_status(
    client: &impl GenericClient,
    actor: &str,
    link: &RemoteLink,
    locked: Option<&BTreeSet<String>>,
) -> Result<Value> {
    let (mut status, target) = link_state(client, actor, link).await?;
    if let Some(address) = target {
        status["satisfied"] =
            json!(target_ready(client, actor, &link.workspace, &address, locked).await?);
    }
    Ok(status)
}

/// The blocking remote targets of a live claim that count as green for this actor.
pub(super) async fn green_targets(
    client: &impl GenericClient,
    actor: &str,
    state: &Workspace,
    locked: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut green = BTreeSet::new();
    for link in state
        .remote_links
        .values()
        .flat_map(|l| &l.links)
        .filter(|l| l.blocking)
    {
        if link_status(client, actor, link, Some(locked)).await?["satisfied"] == true {
            green.insert(link.node());
        }
    }
    Ok(green)
}

/// Remote links and a child workspace on one object read, with the access of the reader.
pub(crate) async fn decorate(
    client: &impl GenericClient,
    workspace: &str,
    actor: &str,
    object: &mut Value,
) -> Result<()> {
    let Some(address) = object["address"].as_str().map(str::to_owned) else {
        return Ok(());
    };
    let stored = super::remote::stored_links(client, workspace, &address).await?;
    object["remote_links_revision"] = json!(stored.revision);
    let mut out = vec![];
    let mut review = false;
    for link in &stored.links {
        let mut item = json!(link);
        item["status"] = link_status(client, actor, link, None).await?;
        review |= item["status"]["review_required"] == true;
        out.push(item);
    }
    object["remote_links"] = json!(out);
    // A changed or archived target is shown, never ignored.
    object["remote_review_required"] = json!(review);
    if let Some(child) = object["document"]["data"]["workspace"]
        .as_str()
        .map(str::to_owned)
    {
        object["child_workspace"] = child_summary(client, actor, &child).await?;
    }
    // The immutable ID that remote links and child nodes store; `workspace` is the name.
    object["workspace_key"] = json!(workspace);
    Ok(())
}

/// The rollup of a child workspace for this reader: display only, never evidence.
pub(crate) async fn child_summary(
    client: &impl GenericClient,
    actor: &str,
    child: &str,
) -> Result<Value> {
    if access::check(client, child, actor, false).await.is_err() {
        return Ok(json!({"workspace": child, "access": false, "status": "grey"}));
    }
    let (green, red, total, _) = criteria(client, child, None).await?;
    let status = if red > 0 {
        "red"
    } else if total > 0 && green == total {
        "green"
    } else {
        "grey"
    };
    Ok(json!({"workspace": child, "access": true, "status": status,
        "criteria": {"green": green, "red": red, "total": total}}))
}
