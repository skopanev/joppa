//! The dependency facts of one workspace (requirement:w17jbz70). One flat query
//! reads each `depends_on` edge with the state of its two nodes, the graph walk runs here
//! (dependency_graph.rs), and one query for each tree level finds the descendants of the
//! blocked nodes. No query has a recursive part, so the planner sees the true small sizes.
//!
//! - A target is satisfied when it is missing, when it is archived and its type has no status
//!   rule, when its data with the computed values contains the `done` rule of its type, or when
//!   its type has a status rule and its tone is green.
//! - A node is blocked when a `depends_on` target of the node or of an ancestor is not satisfied.
//! - `dependents` counts the present open nodes that wait on a present open node through a chain
//!   of `depends_on` edges. A satisfied node in the chain stops it.
//!
//! The type rules come from the board projection (`first_view->'object_types'`), so no type
//! name or field name of a workspace is in the code.
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tokio_postgres::GenericClient;

#[path = "dependency_graph.rs"]
mod graph;
pub use graph::{Edge, Node, walk};

/// The dependency facts: the blocked nodes and the count of the dependents of each node with
/// at least one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub blocked: BTreeSet<String>,
    pub dependents: BTreeMap<String, i64>,
}

impl Facts {
    /// True when the node at `address` is blocked.
    pub fn is_blocked(&self, address: &str) -> bool {
        self.blocked.contains(address)
    }

    /// The number of nodes that wait on the node at `address`.
    pub fn count(&self, address: &str) -> i64 {
        self.dependents.get(address).copied().unwrap_or(0)
    }

    /// Set `blocked` and `dependents` on each item with an address.
    pub fn decorate(&self, items: &mut [Value]) {
        for item in items {
            let Some(address) = item["address"].as_str().map(str::to_owned) else {
                continue;
            };
            item["blocked"] = json!(self.is_blocked(&address));
            item["dependents"] = json!(self.count(&address));
        }
    }

    /// The facts as JSON text {address: {blocked, dependents}}, the parameter of the order of
    /// a list (dependency_order.rs).
    pub fn json(&self) -> String {
        let keys = self.blocked.iter().chain(self.dependents.keys());
        let map: serde_json::Map<String, Value> = keys
            .map(|a| {
                let facts = json!({"blocked": self.is_blocked(a), "dependents": self.count(a)});
                (a.clone(), facts)
            })
            .collect();
        Value::Object(map).to_string()
    }
}

/// The facts of `workspace`. With `archived`, an archived node can be blocked too.
pub async fn facts(client: &impl GenericClient, workspace: &str, archived: bool) -> Result<Facts> {
    let node = |present: Option<bool>, satisfied: Option<bool>| {
        present.map(|present| Node {
            present,
            satisfied: satisfied.unwrap_or(false),
        })
    };
    let rows = client.query(EDGES.as_str(), &[&workspace]).await?;
    let edges: Vec<Edge> = rows
        .iter()
        .map(|r| Edge {
            source: r.get(0),
            target: r.get(1),
            waiter: node(r.get(2), r.get(3)),
            target_node: node(r.get(4), r.get(5)),
        })
        .collect();
    let (direct, dependents) = walk(&edges, archived);
    let mut blocked = direct.clone();
    let mut level: Vec<String> = direct.into_iter().collect();
    // Each step reads the children of the last level. A child names its parent by its address
    // or by an old alias of it. A tree has no cycle, and `blocked` stops a repeated node too.
    while !level.is_empty() {
        let rows = client
            .query(CHILDREN, &[&workspace, &level, &archived])
            .await?;
        level = (rows.iter().map(|r| r.get::<_, String>(0)))
            .filter(|child| blocked.insert(child.clone()))
            .collect();
    }
    Ok(Facts {
        blocked,
        dependents,
    })
}

/// The children of the nodes $2 of workspace $1, also archived ones with $3.
const CHILDREN: &str = "SELECT c.address FROM joppa.objects c WHERE c.workspace=$1 AND (c.present OR $3)
    AND c.parent IN (SELECT unnest($2::text[]) UNION SELECT a.address FROM joppa.object_aliases a WHERE a.workspace=$1 AND a.target=ANY($2::text[]))";

/// Each `depends_on` edge of workspace $1 with the presence and the satisfaction of its source
/// and of its target. A missing node has null facts.
static EDGES: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "WITH types AS (
            SELECT k.key AS kind,COALESCE(k.value->>'status','none')<>'none' AS status,k.value->'done' AS done
            FROM joppa.board_cache b CROSS JOIN LATERAL jsonb_each(b.first_view->'object_types') k WHERE b.workspace=$1
        ), edges AS (
            SELECT DISTINCT COALESCE(sa.target,l.source) AS source,COALESCE(ta.target,l.target) AS target
            FROM joppa.object_links l
            LEFT JOIN joppa.object_aliases sa ON sa.workspace=l.workspace AND sa.address=l.source
            LEFT JOIN joppa.object_aliases ta ON ta.workspace=l.workspace AND ta.address=l.target
            WHERE l.workspace=$1 AND l.relation='depends_on'
        ), nodes AS (
            SELECT o.address,o.present,CASE
                WHEN NOT o.present THEN NOT COALESCE(t.status,false)
                WHEN jsonb_typeof(t.done)='object' AND {data} @> t.done THEN true
                WHEN COALESCE(t.status,false) THEN {tone}='green'
                ELSE false END AS satisfied
            FROM joppa.objects o LEFT JOIN types t ON t.kind=o.kind
            WHERE o.workspace=$1 AND o.address IN (SELECT target FROM edges UNION SELECT source FROM edges)
        )
        SELECT e.source,e.target,s.present,s.satisfied,t.present,t.satisfied
        FROM edges e LEFT JOIN nodes s ON s.address=e.source LEFT JOIN nodes t ON t.address=e.target",
        data = crate::computed_fields::DATA_SQL,
        tone = crate::node_filter::TONE,
    )
});

#[cfg(test)]
#[path = "dependency_sql_tests.rs"]
mod tests;
