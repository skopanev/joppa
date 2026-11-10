//! The relations of a node: one key for each relation of the registry in the tree schema, with
//! an optional title of the node for that relation and the edges of the key. depends_on makes
//! the node wait until the target is green and refuses cycles, related blocks nothing, and
//! each other key is a flow: it allows cycles, blocks nothing and resets no criterion.
//!
//! An older event and an older stored revision hold `links`. They map by the replay rule: a
//! blocking depends_on is depends_on, every other link is related. The edges keep the order
//! of the links, and the first link to a target in a key gives its edge and its pin.
use crate::objects::{ObjectLink, ObjectRevision};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[path = "relations_check.rs"]
mod check;
pub use check::*;
#[path = "relations_color.rs"]
mod colors;
pub use colors::*;
#[path = "relations_schema.rs"]
mod schema;
pub use schema::*;
#[path = "relations_stored.rs"]
mod stored;
pub use stored::*;

pub const DEPENDS_ON: &str = "depends_on";
pub const RELATED: &str = "related";

/// The relations of one node by key.
pub type Relations = BTreeMap<String, Relation>;

/// One key of the relations of a node. A key without edges stays: it counts as used. The
/// colour of the key and its facts (addresses of objects that the step shows) are optional,
/// and a stored key without them reads without them.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "color"
    )]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<String>,
}

/// One edge: the target, the condition of a flow step, the pinned target revision and the
/// colour of the edge.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(from = "EdgeInput")]
pub struct Edge {
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// An edge as a write sends it. A detail read gives each edge with the title, the current
/// revision and the presence of its target; a write takes that edge back and ignores them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeInput {
    to: String,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    revision: Option<u32>,
    #[serde(default, deserialize_with = "color")]
    color: Option<String>,
    #[serde(default, rename = "title")]
    _title: serde::de::IgnoredAny,
    #[serde(default, rename = "current_revision")]
    _current_revision: serde::de::IgnoredAny,
    #[serde(default, rename = "present")]
    _present: serde::de::IgnoredAny,
}

impl From<EdgeInput> for Edge {
    fn from(e: EdgeInput) -> Self {
        Edge {
            to: e.to,
            when: e.when,
            revision: e.revision,
            color: e.color,
        }
    }
}

/// The relations of older links by the replay rule.
pub fn from_links(links: &[ObjectLink]) -> Relations {
    let mut out = Relations::new();
    for link in links {
        let key = if link.blocking && link.relation == DEPENDS_ON {
            DEPENDS_ON
        } else {
            RELATED
        };
        let edges = &mut out.entry(key.to_owned()).or_default().edges;
        if !edges.iter().any(|e| e.to == link.target) {
            edges.push(Edge {
                to: link.target.clone(),
                when: None,
                revision: link.revision,
                color: None,
            });
        }
    }
    out
}

/// The older link form of relations: the depends_on edges as blocking links, then the related
/// edges. A flow has no older form.
pub fn to_links(relations: &Relations) -> Vec<ObjectLink> {
    let link = |key: &str, e: &Edge| ObjectLink {
        target: e.to.clone(),
        relation: key.to_owned(),
        revision: e.revision,
        blocking: key == DEPENDS_ON,
    };
    [DEPENDS_ON, RELATED]
        .into_iter()
        .flat_map(|key| edges_of(relations, key).map(move |e| link(key, e)))
        .collect()
}

/// The edges of one key.
pub fn edges_of<'a>(relations: &'a Relations, key: &str) -> impl Iterator<Item = &'a Edge> {
    relations.get(key).into_iter().flat_map(|r| &r.edges)
}

/// A flow: a key other than depends_on and related.
pub fn flow(key: &str) -> bool {
    key != DEPENDS_ON && key != RELATED
}

/// The links of a revision that an event before the epoch of relations_reset saved, in the
/// form and the order of the event. Only the resets of such an event read them, so the replay
/// of an older journal resets the same criteria. No stored row holds them, and two revisions
/// compare equal without them.
#[derive(Clone, Debug, Default)]
pub struct LinkForm(pub Option<Vec<ObjectLink>>);

impl PartialEq for LinkForm {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl ObjectRevision {
    /// The older link form: the links of the event, or the form of the relations.
    pub fn link_form(&self) -> Vec<ObjectLink> {
        self.older_links
            .0
            .clone()
            .unwrap_or_else(|| to_links(&self.relations))
    }

    /// The edges that the status rule links, from: links, the source reset and the inbox
    /// count: depends_on and related, never a flow.
    pub fn counted(&self) -> impl Iterator<Item = &Edge> {
        edges_of(&self.relations, DEPENDS_ON).chain(edges_of(&self.relations, RELATED))
    }

    /// The links and the relations of an inner save that keeps them: the older links of a
    /// revision that keeps their form, or else the whole relations.
    pub fn save_fields(&self) -> (Option<Vec<ObjectLink>>, Option<Patch>) {
        match &self.older_links.0 {
            Some(links) => (Some(links.clone()), None),
            None => {
                let whole = self.relations.iter();
                let whole = whole.map(|(key, r)| (key.clone(), Some(r.clone())));
                (None, Some(whole.collect()))
            }
        }
    }

    /// The depends_on edges.
    pub fn depends_on(&self) -> impl Iterator<Item = &Edge> {
        edges_of(&self.relations, DEPENDS_ON)
    }

    /// Every edge with its key.
    pub fn edges(&self) -> impl Iterator<Item = (&str, &Edge)> {
        self.relations
            .iter()
            .flat_map(|(key, r)| r.edges.iter().map(move |e| (key.as_str(), e)))
    }

    /// Add a related edge to `target`, as an older migration adds a related link. With `any`,
    /// a counted edge of any key to the target is enough; without it, only a related one.
    pub fn link_related(&mut self, target: &str, any: bool) {
        if let Some(links) = &mut self.older_links.0 {
            let found = links
                .iter()
                .any(|l| l.target == target && (any || l.relation == RELATED));
            if !found {
                links.push(ObjectLink {
                    target: target.to_owned(),
                    relation: RELATED.into(),
                    revision: None,
                    blocking: false,
                });
            }
            self.relations = from_links(links);
            return;
        }
        let found = if any {
            self.counted().any(|e| e.to == target)
        } else {
            edges_of(&self.relations, RELATED).any(|e| e.to == target)
        };
        if !found {
            let edges = &mut self.relations.entry(RELATED.into()).or_default().edges;
            edges.push(Edge {
                to: target.to_owned(),
                when: None,
                revision: None,
                color: None,
            });
        }
    }
}
