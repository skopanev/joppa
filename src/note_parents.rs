//! An inbox object has no parent: it links to the objects that it informs. The event
//! migrate_note_parents runs once in each workspace. It moves each inbox object that has a
//! parent to the root, links it to the old parent with relation related, and keeps every
//! revision. Then each inbox type gets parents [].
use crate::model::Workspace;
use crate::objects::{ObjectLink, schema};
use anyhow::{Result, bail, ensure};
use std::collections::BTreeSet;

pub const REASON: &str = "An inbox input has no parent: it links to the object that it informs";

/// The error of a save or a schema change that gives an inbox object a parent.
pub const REFUSAL: &str = "an inbox object has no parent: send parent null and links [{\"target\":\"kind:id\",\"relation\":\"related\"}]";

fn inbox_kinds(w: &Workspace) -> BTreeSet<String> {
    schema(w)
        .types
        .into_iter()
        .filter(|(_, t)| t.inbox == Some(true))
        .map(|(kind, _)| kind)
        .collect()
}

/// After the migration, an inbox object has no parent.
pub fn check_save(w: &Workspace, kind: &str, parent: Option<&str>) -> Result<()> {
    if w.note_parents_migrated && parent.is_some() && inbox_kinds(w).contains(kind) {
        bail!(REFUSAL);
    }
    Ok(())
}

/// After the migration, a type with inbox true has no parent rule.
pub fn check_schema(
    w: &Workspace,
    types: &std::collections::BTreeMap<String, crate::objects::TreeType>,
) -> Result<()> {
    if !w.note_parents_migrated {
        return Ok(());
    }
    for (kind, t) in types {
        ensure!(
            t.inbox != Some(true) || t.parents.is_empty(),
            "{kind}: {REFUSAL}"
        );
    }
    Ok(())
}

/// Apply migrate_note_parents once. The workspace owner is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.note_parents_migrated {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    let kinds = inbox_kinds(w);
    let mut next = schema(w);
    let schema_changed = next
        .types
        .iter()
        .any(|(kind, t)| kinds.contains(kind) && !t.parents.is_empty());
    let schema_revision = if schema_changed {
        next.revision + 1
    } else {
        next.revision
    };
    let moves: Vec<(String, String)> = w
        .objects
        .iter()
        .filter(|(_, o)| kinds.contains(&o.kind))
        .filter_map(|(key, o)| Some((key.clone(), o.current().parent.clone()?)))
        .collect();
    for (key, parent) in moves {
        let o = w.objects.get_mut(&key).expect("listed object");
        let mut revision = o.current().clone();
        revision.parent = None;
        if !revision.links.iter().any(|l| l.target == parent) {
            revision.links.push(ObjectLink {
                target: parent,
                relation: "related".into(),
                revision: None,
                blocking: false,
            });
        }
        revision.revision += 1;
        revision.contract = None;
        revision.schema_revision = schema_revision;
        revision.position = position;
        revision.actor = actor.into();
        revision.reason = REASON.into();
        revision.at = at.into();
        o.revisions.push(revision);
    }
    if schema_changed {
        for (kind, t) in next.types.iter_mut() {
            if kinds.contains(kind) {
                t.parents.clear();
                t.root = true;
            }
        }
        next.revision = schema_revision;
        next.actor = actor.into();
        next.at = at.into();
        next.position = position;
        next.reason = REASON.into();
        w.tree_schema.push(next);
    }
    w.note_parents_migrated = true;
    crate::objects::validate_tree(w)
}
