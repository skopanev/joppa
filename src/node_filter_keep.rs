//! Part of the node filter SQL: the filters that keep a set of addresses, as a parent does.
//! not_data, has_tests and comment_prefix read the kept addresses, and blocked reads the
//! blocked nodes (dependency_sql.rs).
use crate::node_filter::NodeFilter;
use anyhow::Result;
use std::collections::BTreeSet;
use tokio_postgres::GenericClient;

/// The descendants that the filters keep, and true when the read drops the addresses instead.
/// `blocked: false` without other kept addresses drops the blocked nodes.
pub(super) async fn narrow(
    client: &impl GenericClient,
    workspace: &str,
    filter: &NodeFilter,
    archived: bool,
    descendants: Option<Vec<String>>,
) -> Result<(Option<Vec<String>>, bool)> {
    let descendants = match filter.keeps() {
        None => descendants,
        Some((not_data, has_tests, prefix)) => {
            let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
                [&workspace, &archived, not_data, has_tests, prefix];
            let rows = client.query(KEEPS.as_str(), &params).await?;
            let kept: BTreeSet<String> = rows.iter().map(|row| row.get(0)).collect();
            Some(within(descendants, &kept, true))
        }
    };
    let Some(wanted) = filter.blocked else {
        return Ok((descendants, false));
    };
    let blocked = crate::dependency_sql::facts(client, workspace, archived).await?;
    Ok(match descendants {
        Some(_) => (Some(within(descendants, &blocked.blocked, wanted)), false),
        None => (Some(blocked.blocked.into_iter().collect()), !wanted),
    })
}

/// The addresses of `descendants`, or of `set` without descendants, whose presence in `set` is
/// `wanted`.
fn within(descendants: Option<Vec<String>>, set: &BTreeSet<String>, wanted: bool) -> Vec<String> {
    match descendants {
        Some(below) => below
            .into_iter()
            .filter(|a| set.contains(a) == wanted)
            .collect(),
        None => set.iter().cloned().collect(),
    }
}

/// The addresses that not_data ($3), has_tests ($4) and comment_prefix ($5) keep.
static KEEPS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!("SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND (o.present OR $2)
        AND ($3::text IS NULL OR NOT ({} @> $3::text::jsonb))
        AND ($4::boolean IS NULL OR $4=EXISTS(SELECT 1 FROM joppa.objects t WHERE t.workspace=o.workspace AND t.parent=o.address AND t.present AND t.kind IN (SELECT k.key FROM joppa.board_cache b CROSS JOIN LATERAL jsonb_each(b.first_view->'object_types') k WHERE b.workspace=o.workspace AND k.value->>'role'='test')))
        AND ($5::text IS NULL OR EXISTS(SELECT 1 FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'comments') c WHERE n.workspace=o.workspace AND n.section='notes' AND n.key=o.address AND left(c.key,length($5))=$5 AND NOT COALESCE((c.value->>'withdrawn')::boolean,false)))", crate::computed_fields::DATA_SQL)
});
