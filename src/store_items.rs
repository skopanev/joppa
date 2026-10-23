//! The workspace list, the gate check and reads by a global item ID.
use super::*;

pub async fn list(pool: &Pool, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let allowed = access::scope(&**conn, actor).await?;
    let rows = conn
        .query(
            &format!("{} ORDER BY workspace", access::MEMBERSHIP),
            &[&actor],
        )
        .await?;
    let mut items = vec![];
    for row in rows {
        let id: String = row.get(0);
        if allowed.as_ref().is_some_and(|set| !set.contains(&id)) {
            continue;
        }
        if let Some(card) = conn.query_opt("SELECT first_view->>'workspace_id',first_view->>'title' FROM joppa.board_cache WHERE workspace=$1 AND first_view->'members' ? $2", &[&id,&actor]).await? {
            items.push(json!({"id": card.get::<_,String>(0), "title": card.get::<_,String>(1)}));
        }
    }
    Ok(json!(items))
}

/// The deployment caller supplies the exact target. Accepted staging evidence
/// must never authorize a different production build or provider configuration.
pub fn gate(card: &Value, subject: &std::collections::BTreeMap<String, String>) -> Result<()> {
    ensure!(
        card["launch"]["ready"] == true,
        "launch conditions are absent or not accepted"
    );
    let state: Workspace = serde_json::from_value(card["state"].clone())?;
    for key in crate::objects::launch_objects(&state) {
        let (_, r) = crate::objects::verification_context(&state, key)
            .context("launch condition has no verification context")?;
        let (_, run) = checks::latest(r).ok_or_else(|| anyhow::anyhow!("no verified subject"))?;
        ensure!(
            run.subject
                .iter()
                .all(|(key, value)| subject.get(key) == Some(value)),
            "launch target differs from the verified subject for {}",
            r.title
        );
    }
    Ok(())
}

/// Resolve a globally unique reference without exposing another workspace's content.
pub async fn read_item(pool: &Pool, id: &str, actor: &str) -> Result<Value> {
    read_item_scoped(pool, id, actor, Scope::from_args(&json!({}))?).await
}

pub async fn read_item_scoped(
    pool: &Pool,
    id: &str,
    actor: &str,
    mut scope: Scope,
) -> Result<Value> {
    ensure!(
        scope.req.is_none() && scope.target.is_none(),
        "item_id cannot be combined with req or target"
    );
    // An address has a colon; a system ID never has one. The shape alone decides,
    // so this error tells nothing about other workspaces.
    ensure!(
        !id.contains(':'),
        "item_id takes a copied system ID such as joppa-A-WY86JU3XHX. For an address such as {id}, use joppa_objects {{workspace, address}} or joppa_read {{workspace, target}}"
    );
    let conn = pool.get().await?;
    let row = conn
        .query_opt(
            "SELECT workspace,kind,local_id FROM joppa.item_ids WHERE id=$1",
            &[&id],
        )
        .await?
        .ok_or(AccessDenied)?;
    let workspace: String = row.get(0);
    let kind: String = row.get(1);
    let local_id: String = row.get(2);
    access::check(&**conn, &workspace, actor, false).await?;
    drop(conn);
    let address = format!("{kind}:{local_id}");
    let mut result = read_objects(
        pool,
        &workspace,
        actor,
        &ObjectScope {
            address: Some(&address),
            ..Default::default()
        },
    )
    .await?;
    if let Some(req) = result["process"]["req"].as_str() {
        scope.req = Some(req.into());
        let mut evidence = read(pool, &workspace, actor, &scope).await?;
        evidence["object"] = if scope.revision.is_some()
            || scope.history
            || result["document"]["present"] == false
        {
            json!({"address":result["address"],"kind":result["kind"],"id":result["id"],"item_id":result["item_id"],"current_revision":result["current_revision"],"revision_count":result["revision_count"],"metadata_scope":"current"})
        } else {
            result.clone()
        };
        if scope.revision.is_none() && !scope.history {
            evidence["process"] = result["process"].clone();
        }
        let r = &evidence["requirement"]["revisions"][0];
        let criterion = result["process"]["criterion"].as_str();
        evidence["selection"] = json!({"kind":kind,"id":local_id,"system_id":id,"req":req,"revision":r["number"],"present":criterion.is_none_or(|ac|r["acs"].as_array().is_some_and(|acs|acs.iter().any(|a|a["id"]==ac)))});
        return Ok(evidence);
    }
    if scope.revision.is_some() || scope.history {
        result = read_objects(
            pool,
            &workspace,
            actor,
            &ObjectScope {
                address: Some(&address),
                revision: scope.revision,
                history: scope.history,
                ..Default::default()
            },
        )
        .await?;
    }
    result["selection"] = json!({"kind":result["kind"],"id":result["id"],"system_id":id});
    Ok(result)
}
