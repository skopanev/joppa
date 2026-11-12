//! History marks: a member marks one open entry, or 1 to 25 open entries, processed with a
//! reason and nodes. A mark of several entries marks all of them or none.
use super::super::*;
use serde::Deserialize;

/// The largest node list of one mark.
pub const MARK_NODES_MAX: usize = 15;

/// The largest position list of one mark: the page size of an open History read.
pub const MARK_POSITIONS_MAX: usize = 25;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Mark {
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    position: Option<i64>,
    /// The entries of the mark: `[position]`, or the sent `positions` after the parse.
    #[serde(default)]
    positions: Option<Vec<i64>>,
    reason: String,
    #[serde(default)]
    nodes: Vec<String>,
}

/// Check a mark before any read of the workspace. One of position and positions names the
/// entries, each one once.
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
    let mut mark: Mark = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("invalid history mark: {e}"))?;
    let _ = &mark.workspace;
    let positions = match (mark.position, &mark.positions) {
        (Some(position), None) => vec![position],
        (None, Some(list)) => {
            ensure!(
                (1..=MARK_POSITIONS_MAX).contains(&list.len()),
                "positions holds 1 to 25 entries"
            );
            list.clone()
        }
        _ => anyhow::bail!("send position or positions"),
    };
    ensure!(
        positions.iter().all(|p| *p >= 1),
        "position must be a journal position"
    );
    let unique: std::collections::BTreeSet<&i64> = positions.iter().collect();
    ensure!(
        unique.len() == positions.len(),
        "positions names each entry once"
    );
    // A sent list keeps the reply with positions; one position keeps the reply with position.
    if mark.positions.is_some() {
        mark.positions = Some(positions);
    }
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

/// Mark open entries processed. The server sets the actor and the time. A refused mark marks
/// nothing.
pub async fn mark_history(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    let mut mark = parse_mark(args)?;
    let positions = match (&mark.positions, mark.position) {
        (Some(list), _) => list.clone(),
        (None, position) => position.into_iter().collect(),
    };
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
    let missing = tx
        .query_opt(
            "SELECT x.p FROM unnest($2::bigint[]) WITH ORDINALITY x(p,i) WHERE NOT EXISTS(SELECT 1 FROM joppa.events WHERE workspace=$1 AND position=x.p) ORDER BY x.i LIMIT 1",
            &[&id, &positions],
        )
        .await?;
    if let Some(row) = missing {
        anyhow::bail!("position {} is not a journal entry", row.get::<_, i64>(0));
    }
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
    let rows = tx.query(
        "INSERT INTO joppa.history_marks(workspace,position,actor,reason,nodes,alias) SELECT $1,p,$3,$4,$5::text::jsonb,$6 FROM unnest($2::bigint[]) p ON CONFLICT (workspace,position) DO NOTHING RETURNING marked_at::text",
        &[&id, &positions, &actor, &mark.reason, &nodes, &alias],
    ).await?;
    // The transaction ends without a commit, so a refused mark marks no entry.
    let (true, Some(row)) = (rows.len() == positions.len(), rows.first()) else {
        return Err(commands::ClientField("the entry is already processed").into());
    };
    let at = row.get::<_, String>(0);
    tx.commit().await?;
    let mut reply = match &mark.positions {
        Some(positions) => {
            json!({"positions": positions, "status": "processed", "actor": actor, "at": at, "nodes": mark.nodes})
        }
        None => {
            json!({"position": mark.position, "status": "processed", "actor": actor, "at": at, "reason": mark.reason, "nodes": mark.nodes})
        }
    };
    crate::actor_alias::put(&mut reply, alias.as_deref());
    Ok(reply)
}
