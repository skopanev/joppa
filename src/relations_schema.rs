//! Part of `relations`: the registry of relations in the tree schema. Every workspace has the
//! system relations depends_on and related, also in a schema revision that was stored before
//! the registry. set_tree_schema adds or replaces relations with put_relations and removes
//! them with remove_relations; a command without them keeps the registry.
use super::*;
use crate::model::Workspace;
use anyhow::{Context, Result, ensure};

/// One relation of the registry. A put replaces the title and the description.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelationType {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// True for depends_on and related. Only the server sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<bool>,
}

/// The registry of a schema revision without one: the system relations.
pub fn system() -> BTreeMap<String, RelationType> {
    let system = |title: &str, description: &str| RelationType {
        title: title.into(),
        description: description.into(),
        system: Some(true),
    };
    BTreeMap::from([
        (
            DEPENDS_ON.to_owned(),
            system("Depends on", "The node depends on the target. No cycles."),
        ),
        (
            RELATED.to_owned(),
            system("Related", "A reference between nodes that blocks nothing."),
        ),
    ])
}

/// The registry of the next schema revision after `put` and `remove`.
pub fn requested(
    w: &Workspace,
    put: Option<&BTreeMap<String, RelationType>>,
    remove: Option<&Vec<String>>,
) -> Result<BTreeMap<String, RelationType>> {
    let mut registry = crate::objects::schema(w).relations;
    for (key, r) in put.into_iter().flatten() {
        crate::commands::identifier(key)?;
        ensure!(r.system.is_none(), "system is set by the server");
        ensure!(
            r.title.chars().count() <= TEXT_MAX,
            "title holds at most {TEXT_MAX} characters"
        );
        let limit = crate::commands::DESCRIPTION_MAX;
        ensure!(
            r.description.chars().count() <= limit,
            "a description is at most {limit} characters"
        );
        let system = registry.get(key).and_then(|old| old.system);
        let (title, description) = (r.title.clone(), r.description.clone());
        let r = RelationType {
            title,
            description,
            system,
        };
        registry.insert(key.clone(), r);
    }
    for key in remove.into_iter().flatten() {
        let old = registry.get(key).context("unknown link relation")?;
        ensure!(
            old.system != Some(true),
            "a system relation cannot be removed"
        );
        let used = w.objects.values().any(|o| {
            let current = o.current();
            current.present && current.relations.contains_key(key)
        });
        ensure!(!used, "relation {key} is used by a live node");
        registry.remove(key);
    }
    Ok(registry)
}

/// A restored node needs each of its relations in the registry.
pub fn check_restore(w: &Workspace, relations: &Relations) -> Result<()> {
    let registry = crate::objects::schema(w).relations;
    for key in relations.keys() {
        ensure!(registry.contains_key(key), "declare relation {key} first");
    }
    Ok(())
}
