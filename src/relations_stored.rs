//! Part of `relations`: the stored form of a revision. joppa.object_revisions keeps each
//! revision as it was stored, and a revision before migration 062 holds `links`. A read of
//! such a revision maps its links by the replay rule, so every read gives relations.
use super::*;
use serde_json::Value;

/// An object revision as a row stores it: with relations, or with the older links.
#[derive(Deserialize)]
pub struct StoredRevision {
    revision: u32,
    schema_revision: u32,
    parent: Option<String>,
    data: Value,
    #[serde(default)]
    relations: Option<Relations>,
    #[serde(default)]
    links: Option<Vec<ObjectLink>>,
    actor: String,
    #[serde(default)]
    alias: Option<String>,
    at: String,
    position: u64,
    reason: String,
    present: bool,
    #[serde(default)]
    contract: Option<[u32; 2]>,
}

impl From<StoredRevision> for ObjectRevision {
    fn from(r: StoredRevision) -> Self {
        let relations = r
            .relations
            .unwrap_or_else(|| from_links(r.links.as_deref().unwrap_or_default()));
        ObjectRevision {
            revision: r.revision,
            schema_revision: r.schema_revision,
            parent: r.parent,
            data: r.data,
            relations,
            older_links: LinkForm(r.links),
            actor: r.actor,
            alias: r.alias,
            at: r.at,
            position: r.position,
            reason: r.reason,
            present: r.present,
            contract: r.contract,
        }
    }
}

/// A stored document as a read gives it: a document with relations stays as stored, and an
/// older document with links gives the relations of the replay rule.
pub fn adapt_document(document: Value) -> serde_json::Result<Value> {
    if document.get("links").is_none() {
        return Ok(document);
    }
    serde_json::to_value(serde_json::from_value::<ObjectRevision>(document)?)
}

/// The relations of a stored document, also of an older one with links.
pub fn relations_value(document: &Value) -> Value {
    match document.get("links") {
        Some(links) if document.get("relations").is_none() => {
            let links: Vec<ObjectLink> = serde_json::from_value(links.clone()).unwrap_or_default();
            serde_json::to_value(from_links(&links)).unwrap_or_default()
        }
        _ => document["relations"].clone(),
    }
}

/// The relation keys that differ between two stored documents.
pub fn changed_keys(saved: &Value, previous: &Value) -> Vec<String> {
    let (new, old) = (relations_value(saved), relations_value(previous));
    let keys = new.as_object().into_iter().chain(old.as_object());
    let mut keys: Vec<String> = keys.flat_map(|m| m.keys().cloned()).collect();
    keys.sort();
    keys.dedup();
    keys.retain(|k| new.get(k) != old.get(k));
    // Two older revisions compare their links: a non-blocking depends_on that became related
    // is a change of the related key.
    fn older(d: &Value) -> Option<&Value> {
        d.get("relations")
            .is_none()
            .then(|| d.get("links"))
            .flatten()
    }
    if keys.is_empty() && older(saved).is_some() && older(saved) != older(previous) {
        keys.push(RELATED.to_owned());
    }
    keys
}

/// The current document of joppa.objects: the revision as the history stores it, with its
/// relations and no links.
pub fn stored_document(revision: &ObjectRevision) -> serde_json::Result<Value> {
    serde_json::to_value(revision)
}
