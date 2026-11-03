//! The SQL of the node filters: the descendants of the parent and one clause on the alias `o`.
use super::NodeFilter;
use anyhow::Result;
use tokio_postgres::GenericClient;
use tokio_postgres::types::ToSql;

/// The filters with the descendants of the parent read from the database.
pub struct Bound {
    workspace: String,
    descendants: Option<Vec<String>>,
    roots: bool,
    filter: NodeFilter,
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
    let tone = crate::status_colors::ARCHIVED_TONE;
    anyhow::ensure!(
        archived || !filter.statuses.iter().flatten().any(|s| s == tone),
        "status {tone} marks archived nodes; find them with joppa_search and all_versions"
    );
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
                .query(
                    "WITH RECURSIVE node(address) AS (
                        SELECT COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2)
                    ), sub(address,depth) AS (
                        SELECT c.address,1 FROM joppa.objects c WHERE c.workspace=$1 AND (c.present OR $4)
                            AND ($2::text IS NULL AND c.parent IS NULL OR c.parent IN (SELECT address FROM node
                                UNION SELECT a.address FROM joppa.object_aliases a JOIN node n ON a.workspace=$1 AND a.target=n.address))
                        UNION
                        SELECT c.address,s.depth+1 FROM sub s JOIN joppa.objects c ON c.workspace=$1 AND (c.present OR $4)
                            AND (c.parent=s.address OR c.parent IN (SELECT address FROM joppa.object_aliases WHERE workspace=$1 AND target=s.address))
                        WHERE $3::int IS NULL OR s.depth<$3
                    ) SELECT DISTINCT address FROM sub",
                    &[&workspace, &parent, &filter.depth, &archived],
                )
                .await?;
            Some(rows.iter().map(|row| row.get(0)).collect())
        }
    };
    Ok(Bound {
        workspace: workspace.to_owned(),
        descendants,
        roots: roots && filter.parent.is_none() && !filter.narrows(),
        filter: filter.clone(),
    })
}

impl Bound {
    /// The SQL clause on the alias `o`, with the parameters `$first` to `$first+8`.
    pub fn clause(&self, first: usize) -> String {
        self.clause_at(std::array::from_fn(|i| first + i))
    }

    /// The SQL clause on the alias `o`, with these numbers for the 9 parameters of `params`.
    pub fn clause_at(&self, numbers: [usize; 9]) -> String {
        let p = numbers.map(|n| format!("${n}"));
        format!(
            "({w}::text IS NULL OR (o.workspace={w} AND o.address=ANY({d}::text[])))
            AND (NOT {r}::boolean OR o.parent IS NULL)
            AND ({k}::text[] IS NULL OR o.kind=ANY({k}))
            AND ({s}::text[] IS NULL OR {TONE}=ANY({s}))
            AND ({ow}::text IS NULL OR o.owner={ow})
            AND ({a}::text IS NULL OR {ASSIGNEE}={a})
            AND ({rd}::boolean IS NULL OR {READY}={rd})
            AND ({data}::text IS NULL OR o.document->'data' @> {data}::text::jsonb)",
            w = p[0],
            d = p[1],
            r = p[2],
            k = p[3],
            s = p[4],
            ow = p[5],
            a = p[6],
            rd = p[7],
            data = p[8],
        )
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
            &self.filter.data,
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
/// A node is ready unless it waits: for a dependency (`work_ready` false) or for the answer
/// that its readiness mark asks for (state `waiting`).
pub const READY: &str = "(COALESCE((o.rollup->>'work_ready')::boolean,o.work_ready) AND COALESCE(o.process->'assessment'->>'state','')<>'waiting')";
