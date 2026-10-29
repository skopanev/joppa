//! The remote nodes of a live claim graph.
use super::*;

/// The remote nodes of a live claim graph: each blocking link of a present source.
pub(crate) fn remote_edges(state: &Workspace) -> BTreeMap<String, Vec<String>> {
    state
        .remote_links
        .iter()
        .filter(|(source, _)| {
            state
                .objects
                .get(*source)
                .is_some_and(|o| o.current().present)
        })
        .map(|(source, links)| {
            (
                source.clone(),
                links
                    .links
                    .iter()
                    .filter(|l| l.blocking)
                    .map(RemoteLink::node)
                    .collect(),
            )
        })
        .collect()
}
