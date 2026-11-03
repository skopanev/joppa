//! The dependency-wait reader over the current projections. It reads the test statuses of
//! the criteria; a status read error is returned.
use super::*;
use crate::dependencies::Graph;
use std::collections::{BTreeMap, BTreeSet};

type Types = BTreeMap<String, crate::objects::TreeType>;

fn green(statuses: BTreeMap<String, Value>) -> BTreeSet<String> {
    statuses
        .into_iter()
        .filter(|(_, s)| s["status"] == "green")
        .map(|(address, _)| address)
        .collect()
}

/// The first unsatisfied target of each waiting live node, and for each the number of its
/// unsatisfied targets.
pub(super) async fn read_with_counts(
    client: &impl GenericClient,
    workspace: &str,
    types: &Types,
) -> Result<(BTreeMap<String, String>, BTreeMap<String, usize>)> {
    crate::probe::count(&crate::probe::WAITS);
    // Every blocking link of a live node takes part, whatever the type of its source.
    let links = client.query("SELECT l.source,COALESCE(a.target,l.target) FROM joppa.object_links l JOIN joppa.objects s ON s.workspace=l.workspace AND s.address=l.source LEFT JOIN joppa.object_aliases a ON a.workspace=l.workspace AND a.address=l.target WHERE l.workspace=$1 AND l.blocking AND l.relation='depends_on' AND s.present", &[&workspace]).await?;
    if links.is_empty() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }
    let mut graph = Graph::default();
    for row in client.query("SELECT o.address,o.kind,COALESCE(a.target,o.parent) FROM joppa.objects o LEFT JOIN joppa.object_aliases a ON a.workspace=o.workspace AND a.address=o.parent WHERE o.workspace=$1 AND o.present", &[&workspace]).await? {
        let kind: String = row.get(1);
        graph.node(row.get(0), row.get(2), types.get(&kind).is_some_and(|t| t.role == "ac"));
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
    let green = green(statuses);
    Ok((graph.waits(&green), graph.wait_counts(&green)))
}

/// The first blocking remote target that is not ready for this actor and that applies to
/// `root`: a link of `root` or of an ancestor, and with `below`, also a link of a node
/// below `root`. Completion and acceptance use it under the same sorted locks as a claim.
pub(super) async fn remote_gate(
    client: &impl GenericClient,
    actor: &str,
    state: &Workspace,
    root: &str,
    below: bool,
    locked: &BTreeSet<String>,
) -> Result<Option<String>> {
    let root = crate::objects::resolve_address(state, root).to_owned();
    let applies = |key: &str| {
        crate::objects::within(state, &root, key)
            || (below && crate::objects::within(state, key, &root))
    };
    for (source, stored) in &state.remote_links {
        if !applies(source)
            || !state
                .objects
                .get(source)
                .is_some_and(|o| o.current().present)
        {
            continue;
        }
        for link in stored.links.iter().filter(|l| l.blocking) {
            ensure!(
                locked.contains(&link.workspace),
                "the remote links changed during this write; send it again"
            );
            if super::remote_status::link_status(client, actor, link, Some(locked)).await?["satisfied"]
                != true
            {
                return Ok(Some(link.node()));
            }
        }
    }
    Ok(None)
}
