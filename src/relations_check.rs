//! Part of `relations`: the checks of the relations that a save sends. The replay runs the
//! same key, target and cycle checks as a live save. Only a live save checks the size limits,
//! so an older journal replays as it was stored.
use super::*;
use crate::model::Workspace;
use crate::objects::resolve_address;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

/// The most relation keys of one node.
pub const KEYS_MAX: usize = 64;
/// The most edges of one key.
pub const EDGES_MAX: usize = 200;
/// The longest node title of a key and the longest `when`, in characters.
pub const TEXT_MAX: usize = 200;

/// The relations of save_object as sent: a key with null removes the key. A sent value that
/// is not an object is refused.
pub type Patch = BTreeMap<String, Option<Relation>>;

/// Read the `relations` of a save. serde calls it only when the key is present, so a null is
/// refused here and an omitted key stays None.
pub fn patch<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Patch>, D::Error> {
    use serde::de::Error;
    let value = serde_json::Value::deserialize(d)?;
    if !value.is_object() {
        return Err(D::Error::custom("relations must be an object"));
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(D::Error::custom)
}

/// The relations after a patch: a sent key replaces its title and edges, a null removes it,
/// and every other key of `current` stays.
pub fn merge(current: &Relations, patch: &Patch) -> Relations {
    let mut out = current.clone();
    for (key, relation) in patch {
        match relation {
            Some(relation) => out.insert(key.clone(), relation.clone()),
            None => out.remove(key),
        };
    }
    out
}

/// The whole relations of a patch that the server completed: no key is null.
pub fn whole(patch: &Patch) -> Relations {
    merge(&Relations::new(), patch)
}

/// The relations and the link form of a save. An event names its whole relations, an older
/// event its links, and an older event without both gives the object none.
pub fn of_save(patch: Option<&Patch>, links: Option<&[ObjectLink]>) -> (Relations, LinkForm) {
    match (patch, links) {
        (Some(patch), _) => (whole(patch), LinkForm(None)),
        (None, Some(links)) => (from_links(links), LinkForm(Some(links.to_vec()))),
        (None, None) => Default::default(),
    }
}

/// The size limits of a live save.
pub fn check_limits(relations: &Relations) -> Result<()> {
    ensure!(
        relations.len() <= KEYS_MAX,
        "a node holds at most {KEYS_MAX} relations"
    );
    for relation in relations.values() {
        ensure!(
            relation.edges.len() <= EDGES_MAX,
            "a relation holds at most {EDGES_MAX} edges"
        );
        let long =
            |text: &Option<String>| text.as_ref().is_some_and(|t| t.chars().count() > TEXT_MAX);
        ensure!(
            !long(&relation.title),
            "title holds at most {TEXT_MAX} characters"
        );
        ensure!(
            !relation.edges.iter().any(|e| long(&e.when)),
            "when holds at most {TEXT_MAX} characters"
        );
    }
    Ok(())
}

/// Check the relations of a save of `key`. `kept` holds the relations of the current
/// revision: an edge of the same key keeps an absent target. A depends_on cycle is refused.
pub fn validate(w: &Workspace, key: &str, relations: &Relations, kept: &Relations) -> Result<()> {
    let registry = crate::objects::schema(w).relations;
    for (name, relation) in relations {
        ensure!(registry.contains_key(name), "unknown link relation");
        let mut unique = BTreeSet::new();
        for edge in &relation.edges {
            ensure!(unique.insert(&edge.to), "duplicate object link");
            let target = w
                .objects
                .get(resolve_address(w, &edge.to))
                .context("unknown linked object in this workspace")?;
            ensure!(
                target.current().present || edges_of(kept, name).any(|e| e.to == edge.to),
                "linked object is absent from the current tree"
            );
            if let Some(r) = edge.revision {
                ensure!(
                    r > 0 && r <= target.current().revision,
                    "unknown linked revision"
                );
            }
        }
    }
    visit(
        w,
        key,
        key,
        relations,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )
}

/// Walk the depends_on edges and the parents from `key`: a node on the stack is a cycle.
fn visit(
    w: &Workspace,
    key: &str,
    changed: &str,
    relations: &Relations,
    stack: &mut BTreeSet<String>,
    done: &mut BTreeSet<String>,
) -> Result<()> {
    let key = resolve_address(w, key);
    if done.contains(key) {
        return Ok(());
    }
    ensure!(stack.insert(key.into()), "dependency cycle");
    let current = if key == changed {
        relations
    } else {
        &w.objects
            .get(key)
            .context("unknown dependency")?
            .current()
            .relations
    };
    for edge in edges_of(current, DEPENDS_ON) {
        visit(w, &edge.to, changed, relations, stack, done)?;
    }
    if let Some(parent) = w
        .objects
        .get(key)
        .and_then(|o| o.current().parent.as_deref())
    {
        visit(w, parent, changed, relations, stack, done)?;
    }
    stack.remove(key);
    done.insert(key.into());
    Ok(())
}
