//! Links from an object of this workspace to an object of another workspace.
//!
//! The journal stores each link with its pinned revision. The state of the target lives
//! in the other journal, so the server reads it at read and claim time and never stores
//! it here. A replay therefore gives the same state.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteLink {
    /// The immutable ID of the target workspace, never an alias.
    pub workspace: String,
    /// The address of the target object in that workspace, kind:id.
    pub target: String,
    pub relation: String,
    /// The target revision that the link was written against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u32>,
    /// A blocking dependency: work below the source waits until the target is green.
    #[serde(default)]
    pub blocking: bool,
}

/// The remote links of one object with their own revision. A write names the revision it
/// read, so two writers cannot replace each other's links unseen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLinks {
    pub revision: u32,
    pub links: Vec<RemoteLink>,
}

impl RemoteLink {
    /// The key of the target in a dependency graph. Local addresses never start with @.
    pub fn node(&self) -> String {
        format!("@{}/{}", self.workspace, self.target)
    }
}

/// Check the shape of the links of one source. The server checks the targets, the access
/// and the cycles against the other workspaces before it applies the command.
pub fn validate(links: &[RemoteLink]) -> Result<()> {
    ensure!(links.len() <= 50, "an object has at most 50 remote links");
    let mut unique = BTreeSet::new();
    for link in links {
        crate::commands::identifier(&link.workspace)?;
        ensure!(
            link.target
                .split_once(':')
                .is_some_and(|(kind, id)| !kind.is_empty() && !id.is_empty()),
            "a remote link target is an address kind:id"
        );
        ensure!(
            crate::objects::LINK_RELATIONS.contains(&link.relation.as_str()),
            "unknown link relation"
        );
        ensure!(
            !link.blocking || link.relation == "depends_on",
            "blocking links must be dependencies"
        );
        ensure!(link.revision != Some(0), "unknown linked revision");
        ensure!(
            !link.blocking || link.revision.is_some(),
            "a blocking remote link needs the target revision that it depends on"
        );
        ensure!(
            unique.insert((&link.workspace, &link.target, &link.relation)),
            "duplicate remote link"
        );
    }
    Ok(())
}
