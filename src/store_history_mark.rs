//! History marks: a member marks one open entry processed with a reason and nodes.
use super::super::*;
use serde::Deserialize;

/// The largest node list of one mark.
pub const MARK_NODES_MAX: usize = 15;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Mark {
    #[serde(default)]
    workspace: Option<String>,
    position: i64,
    reason: String,
    #[serde(default)]
    nodes: Vec<String>,
}

/// Check a mark before any read of the workspace.
pub(super) fn parse_mark(args: &Value) -> Result<Mark> {
    for field in ["actor", "at"] {
        if args.get(field).is_some() {
            return Err(commands::ClientField(if field == "actor" {
                "actor comes from the server; do not supply it"
            } else {
                "at comes from the server; do not supply it"
            })
            .into());
        }
    }
    let mark: Mark = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("invalid history mark: {e}"))?;
    let _ = &mark.workspace;
    ensure!(mark.position >= 1, "position must be a journal position");
    ensure!(!mark.reason.trim().is_empty(), "a mark needs a reason");
    ensure!(
        mark.nodes.len() <= MARK_NODES_MAX,
        "a mark names at most 15 nodes"
    );
    once(&mark.nodes)?;
    Ok(mark)
}

fn once(nodes: &[String]) -> Result<()> {
    let unique: std::collections::BTreeSet<&String> = nodes.iter().collect();
    ensure!(unique.len() == nodes.len(), "a mark names each node once");
    Ok(())
}

/// Mark one open entry processed. The server sets the actor and the time.
pub async fn mark_history(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    let mut mark = parse_mark(args)?;
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let id = resolve_workspace(&*tx, workspace).await?;
    access::check(&*tx, &id, actor, false).await?;
    // A public item ID names its node by its address, also next to that address.
    mark.nodes = super::super::public_ids::node_addresses(&*tx, &id, &mark.nodes).await?;
    once(&mark.nodes)?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&id],
    )
    .await?;
    let exists: bool = tx
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM joppa.events WHERE workspace=$1 AND position=$2)",
            &[&id, &mark.position],
        )
        .await?
        .get(0);
    ensure!(exists, "position {} is not a journal entry", mark.position);
    let found: i64 = tx
        .query_one(
            "SELECT count(*) FROM unnest($2::text[]) x(address) WHERE EXISTS (SELECT 1 FROM joppa.objects o WHERE o.workspace=$1 AND o.address=COALESCE((SELECT a.target FROM joppa.object_aliases a WHERE a.workspace=$1 AND a.address=x.address), x.address))",
            &[&id, &mark.nodes],
        )
        .await?
        .get(0);
    ensure!(
        found as usize == mark.nodes.len(),
        "every node must be an object of the workspace"
    );
    let nodes = serde_json::to_string(&mark.nodes)?;
    let alias = crate::actor_alias::current();
    let row = tx.query_opt(
        "INSERT INTO joppa.history_marks(workspace,position,actor,reason,nodes,alias) VALUES($1,$2,$3,$4,$5::text::jsonb,$6) ON CONFLICT (workspace,position) DO NOTHING RETURNING marked_at::text",
        &[&id, &mark.position, &actor, &mark.reason, &nodes, &alias],
    ).await?;
    let Some(row) = row else {
        return Err(commands::ClientField("the entry is already processed").into());
    };
    tx.commit().await?;
    Ok({
        let mut reply = json!({"position": mark.position, "status": "processed", "actor": actor, "at": row.get::<_, String>(0), "reason": mark.reason, "nodes": mark.nodes});
        crate::actor_alias::put(&mut reply, alias.as_deref());
        reply
    })
}
