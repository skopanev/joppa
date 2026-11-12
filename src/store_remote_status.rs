//! The current state of remote links and child workspaces, read with the access of an
//! actor. Nothing here is stored: each read and each claim computes it again.
use super::*;
use crate::remote_links::RemoteLink;

/// Green, red and total counts of the present criteria of a workspace, from its roots down.
async fn criteria(client: &impl GenericClient, workspace: &str) -> Result<(i64, i64, i64)> {
    let row = client.query_one(
        "WITH RECURSIVE sub(address) AS (SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND o.present AND o.parent IS NULL
           UNION SELECT o.address FROM joppa.objects o JOIN sub ON o.parent=sub.address WHERE o.workspace=$1 AND o.present)
         SELECT count(*) FILTER (WHERE r.ac AND o.test_status->>'status'='green'),count(*) FILTER (WHERE r.ac AND o.test_status->>'status'='red'),count(*) FILTER (WHERE r.ac)
         FROM joppa.objects o JOIN sub ON sub.address=o.address JOIN joppa.board_cache b ON b.workspace=o.workspace
         CROSS JOIN LATERAL (SELECT b.first_view->'object_types'->o.kind->>'role'='ac' AS ac) r
         WHERE o.workspace=$1",
        &[&workspace],
    ).await?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

/// The status of one remote link as the actor sees it now: access, presence and the pin
/// against the target. Each key is always present, null when the actor cannot know it.
pub(crate) async fn link_status(
    client: &impl GenericClient,
    actor: &str,
    link: &RemoteLink,
) -> Result<Value> {
    if access::check(client, &link.workspace, actor, false)
        .await
        .is_err()
    {
        return Ok(
            json!({"access":false,"present":null,"current_revision":null,"review_required":null}),
        );
    }
    let Some((_, current, contract)) = super::remote::target_contract(client, link).await? else {
        // An archived or absent target asks for review.
        return Ok(
            json!({"access":true,"present":false,"current_revision":null,"review_required":true}),
        );
    };
    // A pin stays current from the contract revision to the current revision.
    let stale = link
        .revision
        .is_some_and(|r| i64::from(contract) > i64::from(r));
    Ok(json!({"access":true,"present":true,"current_revision":current,"review_required":stale}))
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
    decorate_with(client, workspace, actor, object, stored).await
}

/// `decorate` with the stored remote links that the caller read, so that a read of more
/// objects reads all their links in one query.
pub(crate) async fn decorate_with(
    client: &impl GenericClient,
    workspace: &str,
    actor: &str,
    object: &mut Value,
    stored: crate::remote_links::RemoteLinks,
) -> Result<()> {
    object["remote_links_revision"] = json!(stored.revision);
    let mut out = vec![];
    let mut review = false;
    for link in &stored.links {
        let mut item = json!(link);
        item["status"] = link_status(client, actor, link).await?;
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
        return Ok(json!({"workspace": child, "access": false, "status": "blue"}));
    }
    let (green, red, total) = criteria(client, child).await?;
    let status = if red > 0 {
        "red"
    } else if total > 0 && green == total {
        "green"
    } else {
        "blue"
    };
    Ok(json!({"workspace": child, "access": true, "status": status,
        "criteria": {"green": green, "red": red, "total": total}}))
}
