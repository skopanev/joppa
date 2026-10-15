//! The dependency-wait reader over the current projections or a live write state.
//! Both read the criterion statuses of GET /tests; a status read error is returned.
use super::*;
use crate::dependencies::Graph;
use std::collections::{BTreeMap, BTreeSet};

type Types = BTreeMap<String, crate::objects::TreeType>;

fn kinds(types: &Types, role: &str) -> Vec<String> {
    types
        .iter()
        .filter(|(_, t)| t.role == role)
        .map(|(kind, _)| kind.clone())
        .collect()
}

fn green(statuses: BTreeMap<String, Value>) -> BTreeSet<String> {
    statuses
        .into_iter()
        .filter(|(_, s)| s["status"] == "green")
        .map(|(address, _)| address)
        .collect()
}

/// The first unsatisfied target of each waiting live node at the current read position.
pub(super) async fn read(
    client: &impl GenericClient,
    workspace: &str,
    types: &Types,
) -> Result<BTreeMap<String, String>> {
    let links = client.query("SELECT l.source,COALESCE(a.target,l.target) FROM joppa.object_links l JOIN joppa.objects s ON s.workspace=l.workspace AND s.address=l.source LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target WHERE l.workspace=$1 AND l.blocking AND l.relation='depends_on' AND s.present AND s.kind<>ALL($2)", &[&workspace, &kinds(types, "task")]).await?;
    if links.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut graph = Graph::default();
    for row in client.query("SELECT o.address,o.kind,COALESCE(a.target,o.parent) FROM joppa.objects o LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.address=o.parent WHERE o.workspace=$1 AND o.present", &[&workspace]).await? {
        let kind: &str = row.get(1);
        graph.node(row.get(0), row.get(2), types.get(kind).is_some_and(|t| t.role == "ac"));
    }
    for row in links {
        graph.link(row.get(0), row.get(1));
    }
    let statuses = super::reports::criterion_test_statuses(
        client,
        workspace,
        &graph.criteria_needed(),
        &BTreeSet::new(),
    )
    .await?;
    Ok(graph.waits(&green(statuses)))
}

/// The same wait in a live write, from the transaction state with earlier batch writes.
pub(super) async fn live(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
) -> Result<BTreeMap<String, String>> {
    let graph = Graph::from_state(state);
    if !graph.has_links() {
        return Ok(BTreeMap::new());
    }
    let statuses = super::reports::state_criterion_statuses(
        client,
        workspace,
        state,
        &graph.criteria_needed(),
    )
    .await?;
    Ok(graph.waits(&green(statuses)))
}
