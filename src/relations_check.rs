//! Part of `relations`: the checks of the relations that a save sends. The replay runs the
//! same key, target, fact and cycle checks as a live save. Only a live save checks the size
//! limits and the colour form, so an older journal replays as it was stored.
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

/// The relations after a patch: a sent key replaces its title, edges, colour and facts, a
/// null removes it, and every other key of `current` stays.
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

/// The relations and the link form of a save. An event names its whole relations, and an
/// older event its links. A live save without both keeps the relations `live` of the node,
/// and an older event without both gives the node none.
pub fn of_save(
    patch: Option<&Patch>,
    links: Option<&[ObjectLink]>,
    live: Option<&Relations>,
) -> (Relations, LinkForm) {
    match (patch, links) {
        (Some(patch), _) => (whole(patch), LinkForm(None)),
        (None, Some(links)) => (from_links(links), LinkForm(Some(links.to_vec()))),
        (None, None) => (live.cloned().unwrap_or_default(), LinkForm(None)),
    }
}

/// The size limits of a live save. As with the field limit, only a new or changed key counts,
/// and only a growing number of keys: an older node over a limit keeps its stored keys.
pub fn check_limits(relations: &Relations, stored: &Relations) -> Result<()> {
    ensure!(
        relations.len() <= KEYS_MAX.max(stored.len()),
        "a node holds at most {KEYS_MAX} relations"
    );
    let changed = relations.iter().filter(|(k, r)| stored.get(*k) != Some(*r));
    for (_, relation) in changed {
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
        let mut colors = relation.edges.iter().map(|e| &e.color);
        ensure!(
            color_form(&relation.color) && colors.all(color_form),
            "{COLOR_FORM}"
        );
        ensure!(
            relation.facts.len() <= EDGES_MAX,
            "a relation holds at most {EDGES_MAX} facts"
        );
    }
    Ok(())
}

/// Check the relations of a save of `key`. `kept` holds the relations of the current
/// revision: an edge of the same key keeps an absent target. A depends_on cycle is refused.
/// A live save compares the targets of each key that it changes by their resolved address,
/// so an alias and its address are a duplicate. The replay and an unchanged key compare the
/// targets as written, as the older release did.
pub fn validate(w: &Workspace, key: &str, relations: &Relations, kept: &Relations) -> Result<()> {
    let registry = crate::objects::schema(w).relations;
    for (name, relation) in relations {
        ensure!(registry.contains_key(name), "unknown link relation");
        let mut unique = BTreeSet::new();
        let resolved = !w.replaying && kept.get(name) != Some(relation);
        for edge in &relation.edges {
            let to = match resolved {
                true => resolve_address(w, &edge.to),
                false => edge.to.as_str(),
            };
            ensure!(unique.insert(to), "duplicate object link");
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
        facts(w, key, &relation.facts, kept.get(name), resolved)?;
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

/// Check the facts of one key of node `key`, as the edge targets: an object of this workspace,
/// present unless `kept`, the stored key, has it already. No fact is the node, and no fact
/// shows twice: by its resolved address when `resolved`, else as written.
fn facts(
    w: &Workspace,
    key: &str,
    facts: &[String],
    kept: Option<&Relation>,
    resolved: bool,
) -> Result<()> {
    let mut unique = BTreeSet::new();
    let node = resolve_address(w, key);
    for fact in facts {
        let address = resolve_address(w, fact);
        let compared = if resolved { address } else { fact.as_str() };
        ensure!(unique.insert(compared), "a fact is listed twice");
        ensure!(address != node, "a node is not its own fact");
        let target = w
            .objects
            .get(address)
            .context("unknown linked object in this workspace")?;
        ensure!(
            target.current().present || kept.is_some_and(|r| r.facts.contains(fact)),
            "linked object is absent from the current tree"
        );
    }
    Ok(())
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
