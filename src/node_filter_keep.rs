//! Part of the node filter SQL: the filters that keep a set of addresses, as a parent does.
//! not_data, has_tests and comment_prefix read the kept addresses, data_any, data_all and
//! data_none read the GIN indexes (node_filter_sets.rs), and blocked reads the blocked nodes
//! (dependency_sql.rs).
use crate::node_filter::{DataSets, NodeFilter, sets};
use anyhow::Result;
use std::collections::BTreeSet;
use tokio_postgres::GenericClient;

/// The nodes that the filters keep: every node without a set, else the nodes of the set, or
/// with `exclude` every node outside it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kept {
    pub set: Option<BTreeSet<String>>,
    pub exclude: bool,
}

impl Kept {
    /// Keep also only the nodes of `set`.
    pub fn keep(self, set: BTreeSet<String>) -> Self {
        let set = match (self.set, self.exclude) {
            (None, _) => set,
            (Some(mut own), false) => {
                own.retain(|a| set.contains(a));
                own
            }
            (Some(own), true) => set.into_iter().filter(|a| !own.contains(a)).collect(),
        };
        Kept {
            set: Some(set),
            exclude: false,
        }
    }

    /// Drop also the nodes of `set`.
    pub fn skip(self, set: BTreeSet<String>) -> Self {
        let (set, exclude) = match (self.set, self.exclude) {
            (None, _) => (set, true),
            (Some(mut own), false) => {
                own.retain(|a| !set.contains(a));
                (own, false)
            }
            (Some(mut own), true) => {
                own.extend(set);
                (own, true)
            }
        };
        Kept {
            set: Some(set),
            exclude,
        }
    }

    /// True when the filters keep the node.
    pub fn contains(&self, address: &str) -> bool {
        (self.set.as_ref()).is_none_or(|set| set.contains(address) != self.exclude)
    }

    /// The set of the kept nodes, when the filters keep only a set.
    fn only(&self) -> Option<&BTreeSet<String>> {
        self.set.as_ref().filter(|_| !self.exclude)
    }
}

/// The nodes that the filters keep within `kept`.
pub(super) async fn narrow(
    client: &impl GenericClient,
    workspace: &str,
    filter: &NodeFilter,
    archived: bool,
    mut kept: Kept,
) -> Result<Kept> {
    if let Some((not_data, has_tests, prefix)) = filter.keeps() {
        let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
            [&workspace, &archived, not_data, has_tests, prefix];
        let rows = client.query(KEEPS.as_str(), &params).await?;
        kept = kept.keep(rows.iter().map(|row| row.get(0)).collect());
    }
    kept = by_sets(client, workspace, &filter.sets, archived, kept).await?;
    let Some(wanted) = filter.blocked else {
        return Ok(kept);
    };
    let blocked = crate::dependency_sql::facts(client, workspace, archived).await?;
    Ok(match wanted {
        true => kept.keep(blocked.blocked),
        false => kept.skip(blocked.blocked),
    })
}

/// The nodes of the workspace that data_any, data_all and data_none keep (node_filter_sets.rs).
pub async fn kept_by_sets(
    client: &impl GenericClient,
    workspace: &str,
    filters: &DataSets,
) -> Result<Kept> {
    by_sets(client, workspace, filters, false, Kept::default()).await
}

/// The nodes that data_any, data_all and data_none keep within `kept`.
pub(super) async fn by_sets(
    client: &impl GenericClient,
    workspace: &str,
    filters: &DataSets,
    archived: bool,
    mut kept: Kept,
) -> Result<Kept> {
    let (keep, skip) = filters.groups();
    if !keep.is_empty() {
        let set = sets::addresses(client, workspace, archived, kept.only(), &keep, " AND ");
        let set = set.await?;
        kept = kept.keep(set);
    }
    if !skip.is_empty() {
        let set = sets::addresses(client, workspace, archived, kept.only(), &skip, " OR ");
        let set = set.await?;
        kept = kept.skip(set);
    }
    Ok(kept)
}

/// The addresses that not_data ($3), has_tests ($4) and comment_prefix ($5) keep.
static KEEPS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!("SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND (o.present OR $2)
        AND ($3::text IS NULL OR NOT ({} @> $3::text::jsonb))
        AND ($4::boolean IS NULL OR $4=EXISTS(SELECT 1 FROM joppa.objects t WHERE t.workspace=o.workspace AND t.parent=o.address AND t.present AND t.kind IN (SELECT k.key FROM joppa.board_cache b CROSS JOIN LATERAL jsonb_each(b.first_view->'object_types') k WHERE b.workspace=o.workspace AND k.value->>'role'='test')))
        AND ($5::text IS NULL OR EXISTS(SELECT 1 FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'comments') c WHERE n.workspace=o.workspace AND n.section='notes' AND n.key=o.address AND left(c.key,length($5))=$5 AND NOT COALESCE((c.value->>'withdrawn')::boolean,false)))", crate::computed_fields::DATA_SQL)
});

#[cfg(test)]
#[path = "node_filter_keep_tests.rs"]
mod tests;
