//! A deleted workspace leaves no reference behind in the workspaces that survive it.
//!
//! The server appends one internal event to each workspace that names the deleted one.
//! The event removes the remote links to it and the child workspace binding of each node.
//! Each change is a normal change of the source: the remote links get a new revision, a
//! node gets a new object revision, and the criteria at and below it turn grey.
use crate::model::Workspace;
use anyhow::{Result, ensure};
use serde_json::Value;

/// The actor of the internal events of a workspace deletion. No person or token has it.
pub const ACTOR: &str = "joppa-deletion";

/// The addresses of the live objects that name `workspace`: remote links or a binding.
pub fn sources(w: &Workspace, workspace: &str) -> Vec<String> {
    let mut found: Vec<String> = w
        .remote_links
        .iter()
        .filter(|(_, l)| l.links.iter().any(|link| link.workspace == workspace))
        .map(|(source, _)| source.clone())
        .chain(bound(w, workspace))
        .collect();
    found.sort();
    found.dedup();
    found
}

fn bound(w: &Workspace, workspace: &str) -> Vec<String> {
    w.objects
        .iter()
        .filter(|(_, o)| {
            o.current().present
                && o.current().data.get("workspace").and_then(Value::as_str) == Some(workspace)
        })
        .map(|(key, _)| key.clone())
        .collect()
}

/// Remove every reference to `workspace` from `w`, as the internal deletion actor.
pub fn remove(
    w: &mut Workspace,
    actor: &str,
    workspace: &str,
    reason: &str,
    position: u64,
    at: &str,
) -> Result<()> {
    ensure!(actor == ACTOR, "internal deletion event required");
    crate::commands::nonempty(reason)?;
    let linked: Vec<String> = w
        .remote_links
        .iter()
        .filter(|(_, l)| l.links.iter().any(|link| link.workspace == workspace))
        .map(|(source, _)| source.clone())
        .collect();
    for source in linked {
        let current = w.remote_links[&source].clone();
        let (gone, kept): (Vec<_>, Vec<_>) = current
            .links
            .into_iter()
            .partition(|link| link.workspace == workspace);
        w.remote_links.insert(
            source.clone(),
            crate::remote_links::RemoteLinks {
                revision: current.revision + 1,
                links: kept,
                position,
            },
        );
        // A removed pin resets the criteria below the source, as set_remote_links does.
        if gone.iter().any(|link| link.revision.is_some()) && w.objects.contains_key(&source) {
            crate::objects::reset_criteria(w, &source, at)?;
        }
    }
    let schema_revision = crate::objects::schema(w).revision;
    for key in bound(w, workspace) {
        let object = w.objects.get_mut(&key).expect("bound object");
        let mut next = object.current().clone();
        if let Some(data) = next.data.as_object_mut() {
            data.remove("workspace");
        }
        next.revision += 1;
        next.schema_revision = schema_revision;
        next.position = position;
        next.actor = actor.to_owned();
        next.reason = reason.to_owned();
        next.at = at.to_owned();
        object.revisions.push(next);
        crate::objects::reset_criteria(w, &key, at)?;
    }
    Ok(())
}
