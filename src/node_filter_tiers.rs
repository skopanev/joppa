//! Part of the node filter SQL: `tiers`, 1 to 10 filter objects in order. A read keeps the
//! nodes that match the other filters and at least one tier. A list gives the nodes of the
//! first tier first, then the nodes of the next tier, and so on. A tier reads every level
//! when it names no parent, so `{}` keeps every node as the last fallback.
use super::{Bound, Kept, NodeFilter, plain};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio_postgres::GenericClient;
use tokio_postgres::types::ToSql;

/// The tier of each node, for the order of a list.
#[derive(Clone, Debug, PartialEq)]
pub struct Ranks {
    /// The JSON object of the address and the tier of each node before the tier `rest`.
    pub json: String,
    /// The tier of the other nodes: the first tier that keeps every node, else the number of
    /// tiers.
    pub rest: i32,
}

/// Read the descendants of the parent, or the nodes within `depth` of the top. `roots` keeps
/// only root nodes when no filter narrows the read. `archived` also walks below archived nodes.
pub async fn bind(
    client: &impl GenericClient,
    workspace: &str,
    filter: &NodeFilter,
    roots: bool,
    archived: bool,
) -> Result<Bound> {
    let mut bound = plain(client, workspace, filter, roots, archived).await?;
    if let Some(each) = &filter.tiers {
        let set = bound.descendants.take().map(|d| d.into_iter().collect());
        let kept = Kept {
            set,
            exclude: bound.exclude,
        };
        let (kept, ranks) = rank(client, workspace, each, archived, kept).await?;
        bound.descendants = kept.set.map(|set| set.into_iter().collect());
        bound.exclude = kept.exclude;
        bound.ranks = Some(ranks);
    }
    Ok(bound)
}

/// The tiers of MCP arguments, or of the JSON text of an HTTP query.
pub fn tiers(a: &Value) -> Result<Option<Vec<NodeFilter>>> {
    let parsed: Value;
    let items = match a.get("tiers") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(items)) => items,
        Some(Value::String(s)) if s.trim_start().starts_with('[') => {
            parsed = serde_json::from_str(s).unwrap_or_default();
            parsed.as_array().context(COUNT)?
        }
        Some(_) => bail!(COUNT),
    };
    ensure!((1..=10).contains(&items.len()), COUNT);
    let tier = |item: &Value| {
        ensure!(item.is_object(), "each tier must be a filter object");
        ensure!(item.get("tiers").is_none(), "tiers cannot nest tiers");
        NodeFilter::parse(item)
    };
    items.iter().map(tier).collect::<Result<_>>().map(Some)
}

const COUNT: &str = "tiers takes 1 to 10 filter objects";

/// The nodes of `kept` that match a tier, and the tier of each node.
pub(super) async fn rank(
    client: &impl GenericClient,
    workspace: &str,
    tiers: &[NodeFilter],
    archived: bool,
    kept: Kept,
) -> Result<(Kept, Ranks)> {
    let mut ranks: BTreeMap<String, usize> = BTreeMap::new();
    let mut rest = None;
    for (i, tier) in tiers.iter().enumerate() {
        let bound = plain(client, workspace, tier, false, archived).await?;
        // A tier that keeps every node ends the tiers.
        if !bound.narrows() {
            rest = Some(i);
            break;
        }
        let sql = format!(
            "SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND (o.present OR $2) AND {}",
            bound.clause(3)
        );
        let mut params: Vec<&(dyn ToSql + Sync)> = vec![&workspace, &archived];
        params.extend(bound.params());
        for row in client.query(&sql, &params).await? {
            ranks.entry(row.get(0)).or_insert(i);
        }
    }
    let kept = match rest {
        Some(_) => kept,
        None => kept.keep(ranks.keys().cloned().collect()),
    };
    let json = serde_json::to_string(&ranks)?;
    let rest = rest.unwrap_or(tiers.len()) as i32;
    Ok((kept, Ranks { json, rest }))
}
