//! The SQL of the node filters: the descendants of the parent and one clause on the alias `o`.
use super::NodeFilter;
use anyhow::Result;
use tokio_postgres::GenericClient;
use tokio_postgres::types::ToSql;
#[path = "node_filter_envelope.rs"]
mod envelope;

/// The filters with the descendants of the parent read from the database.
pub struct Bound {
    workspace: String,
    descendants: Option<Vec<String>>,
    roots: bool,
    filter: NodeFilter,
    /// The data parameter: the data filter, or with a dimension value or a computed field the
    /// envelope {data, computed, combination, key} (filter_dimensions.rs, computed_fields.rs).
    data: Option<String>,
    /// The stored key of the combination that the data filter names.
    pub combination: Option<String>,
    /// True when `data` is the envelope.
    envelope: bool,
    /// The number of computed fields that the data filter names.
    computed: usize,
}

/// The refusal of the archived tone in a read that keeps only present nodes.
pub const ARCHIVED_REFUSAL: &str = "status purple marks archived nodes; find them with joppa_objects, or with joppa_search and all_versions";

/// The descendants of $2, or the top nodes when $2 is null, within depth $3; $4 also walks
/// below archived nodes. A child names its parent by its address or by an old alias of it.
/// Each step joins the parent key by the index on objects(workspace, parent), so a step does
/// not compare each node with each parent.
pub const SUBTREE: &str = "WITH RECURSIVE node(address) AS (
        SELECT COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2)
    ), sub(address,depth) AS (
        SELECT c.address,1 FROM joppa.objects c
            WHERE $2::text IS NULL AND c.workspace=$1 AND c.parent IS NULL AND (c.present OR $4)
        UNION
        SELECT c.address,1 FROM node n
            LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.target=n.address
            CROSS JOIN LATERAL (VALUES (n.address),(a.address)) k(key)
            JOIN joppa.objects c ON c.workspace=$1 AND c.parent=k.key AND (c.present OR $4)
        UNION
        SELECT c.address,s.depth+1 FROM sub s
            LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.target=s.address
            CROSS JOIN LATERAL (VALUES (s.address),(a.address)) k(key)
            JOIN joppa.objects c ON c.workspace=$1 AND c.parent=k.key AND (c.present OR $4)
            WHERE $3::int IS NULL OR s.depth<$3
    ) SELECT DISTINCT address FROM sub";

/// Read the descendants of the parent, or the nodes within `depth` of the top. `roots` keeps
/// only root nodes when no filter narrows the read. `archived` also walks below archived nodes.
pub async fn bind(
    client: &impl GenericClient,
    workspace: &str,
    filter: &NodeFilter,
    roots: bool,
    archived: bool,
) -> Result<Bound> {
    anyhow::ensure!(archived || !filter.archived(), ARCHIVED_REFUSAL);
    // The start of the walk: the parent, or the top of the workspace when only depth is set.
    let parent = match (filter.parent.as_deref(), filter.depth) {
        (Some("*") | None, None) => None,
        (Some("*") | None, Some(_)) => Some(None),
        (Some(parent), _) => Some(Some(parent)),
    };
    let descendants = match parent {
        None => None,
        Some(parent) => {
            let rows = client
                .query(SUBTREE, &[&workspace, &parent, &filter.depth, &archived])
                .await?;
            Some(rows.iter().map(|row| row.get(0)).collect())
        }
    };
    let envelope::Data {
        text: data,
        key: combination,
        envelope,
        computed,
    } = envelope::data(client, workspace, filter.data.as_deref()).await?;
    Ok(Bound {
        workspace: workspace.to_owned(),
        descendants,
        roots: roots && filter.parent.is_none() && !filter.narrows(),
        filter: filter.clone(),
        data,
        combination,
        envelope,
        computed,
    })
}

impl Bound {
    /// The SQL clause on the alias `o`, with the parameters `$first` to `$first+8`.
    pub fn clause(&self, first: usize) -> String {
        self.clause_at(std::array::from_fn(|i| first + i))
    }

    /// The SQL clause on the alias `o`, with these numbers for the 9 parameters of `params`.
    /// The text has only the conditions of the filters that are set, and an unset filter
    /// keeps only a test of its null parameter. So a generic plan of the text also reads the
    /// descendants by the primary key, and no `IS NULL OR` hides an index.
    pub fn clause_at(&self, numbers: [usize; 9]) -> String {
        let [w, d, r, k, s, ow, a, rd, data] = numbers.map(|n| format!("${n}"));
        let f = &self.filter;
        let set = [
            (
                self.descendants.is_some(),
                format!("o.workspace={w} AND o.address=ANY({d}::text[])"),
                format!("{w}::text IS NULL AND {d}::text[] IS NULL"),
            ),
            (
                self.roots,
                format!("{r}::boolean AND o.parent IS NULL"),
                format!("NOT {r}::boolean"),
            ),
            (
                f.kinds.is_some(),
                format!("o.kind=ANY({k}::text[])"),
                format!("{k}::text[] IS NULL"),
            ),
            (
                f.statuses.is_some(),
                match self.combination {
                    Some(_) => format!(
                        "COALESCE(o.dim_tones->>({data}::text::jsonb->>'key'),{TONE})=ANY({s}::text[])"
                    ),
                    None => format!("{TONE}=ANY({s}::text[])"),
                },
                format!("{s}::text[] IS NULL"),
            ),
            (
                f.owner.is_some(),
                format!("o.owner={ow}::text"),
                format!("{ow}::text IS NULL"),
            ),
            (
                f.assignee.is_some(),
                format!("{ASSIGNEE}={a}::text"),
                format!("{a}::text IS NULL"),
            ),
            (
                f.ready.is_some(),
                format!("{READY}={rd}::boolean"),
                format!("{rd}::boolean IS NULL"),
            ),
            (
                self.data.is_some(),
                match self.envelope {
                    // A node applies unless it or an ancestor names other values of a dimension.
                    true => format!(
                        "({data}::text::jsonb->'data' IS NULL OR o.document->'data' @> ({data}::text::jsonb->'data')){} AND NOT EXISTS (SELECT 1 FROM jsonb_each_text({data}::text::jsonb->'combination') c WHERE o.dims ? c.key AND NOT (o.dims->c.key) ? c.value)",
                        crate::computed_fields::filter_clause(&data, self.computed)
                    ),
                    false => format!("o.document->'data' @> {data}::text::jsonb"),
                },
                format!("{data}::text IS NULL"),
            ),
        ];
        let parts: Vec<String> = set
            .into_iter()
            .map(|(on, when, unset)| format!("({})", if on { when } else { unset }))
            .collect();
        parts.join(" AND ")
    }

    /// The 9 parameters of the clause, in its order.
    pub fn params(&self) -> [&(dyn ToSql + Sync); 9] {
        [
            if self.descendants.is_some() {
                &self.workspace
            } else {
                &NONE
            },
            &self.descendants,
            &self.roots,
            &self.filter.kinds,
            &self.filter.statuses,
            &self.filter.owner,
            &self.filter.assignee,
            &self.filter.ready,
            &self.data,
        ]
    }

    /// True when the clause can drop a node.
    pub fn narrows(&self) -> bool {
        self.roots || self.filter.narrows()
    }
}

static NONE: Option<String> = None;

/// The stored tone of a node, as each list item gives it.
pub const TONE: &str = "COALESCE(o.rollup->'signal'->>'tone',o.signal->>'tone')";
/// The assignee of a node: the document field `assignee`.
pub const ASSIGNEE: &str = "(o.document->'data'->>'assignee')";
/// A node is ready unless it waits for a dependency (`work_ready` false).
pub const READY: &str = "COALESCE((o.rollup->>'work_ready')::boolean,o.work_ready)";
