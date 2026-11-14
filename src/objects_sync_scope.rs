//! Part of `objects`: the part of the document ledger that one write changes. The ledger
//! sync (`sync_with`) aligns the objects of the ledger types with the rows of the ledger.
//! Each write ends with a sync, so before a write the objects match the rows. A write that
//! changes only requirements needs the rows of those requirements, and only their old row
//! keys can leave the ledger. Any other ledger change needs every row.
use super::*;
use crate::model::{Capability, Consumer, Domain, Intake, Requirement};
use std::sync::Arc;

/// The ledger inputs other than the requirements.
#[derive(PartialEq)]
struct Rest {
    domains: BTreeMap<String, Domain>,
    capabilities: BTreeMap<String, Capability>,
    consumers: BTreeMap<String, Consumer>,
    projects: BTreeMap<String, String>,
    intake: BTreeMap<String, Intake>,
    admin: String,
    tasks: Vec<String>,
    flags: [bool; 6],
    schema: Option<u32>,
}

impl Rest {
    fn of(w: &Workspace) -> Self {
        Rest {
            domains: w.domains.clone(),
            capabilities: w.capabilities.clone(),
            consumers: w.consumers.clone(),
            projects: w.projects.clone(),
            intake: w.intake.clone(),
            admin: w.admin.clone(),
            tasks: w.task_process.keys().cloned().collect(),
            flags: [
                w.generic_nodes,
                w.documents_migrated,
                w.schema_migrated,
                w.decision_required_migrated,
                w.purpose_migrated,
                w.body_migrated,
            ],
            schema: w.tree_schema.last().map(|s| s.revision),
        }
    }
}

/// The two sets hold the same addresses other than `own`, the object of the write, which
/// `Snapshot::scope` reads.
fn same_set(new: &Arc<BTreeSet<String>>, old: &Arc<BTreeSet<String>>, own: Option<&str>) -> bool {
    Arc::ptr_eq(new, old)
        || new
            .symmetric_difference(old)
            .all(|k| Some(k.as_str()) == own)
}

/// The ledger inputs before a write.
pub(crate) struct Snapshot {
    requirements: BTreeMap<String, Arc<Requirement>>,
    rest: Rest,
    overrides: Arc<BTreeSet<String>>,
    archived: Arc<BTreeSet<String>>,
    own: Option<String>,
}

/// The requirements whose rows a sync builds, and the row keys that they had before.
pub(crate) struct Scope {
    pub requirements: BTreeMap<String, Arc<Requirement>>,
    pub old_keys: BTreeSet<String>,
}

impl Snapshot {
    /// The inputs of `w` before a write of the object `own`.
    pub(crate) fn of(w: &Workspace, own: Option<&str>) -> Self {
        Snapshot {
            requirements: w.requirements.clone(),
            rest: Rest::of(w),
            overrides: w.document_overrides.clone(),
            archived: w.archived_objects.clone(),
            own: own.map(str::to_owned),
        }
    }

    /// The scope of the sync after the write, or None when the sync needs every row.
    pub(crate) fn scope(&self, w: &Workspace) -> Option<Scope> {
        let own = self.own.as_deref();
        if Rest::of(w) != self.rest
            || !same_set(&w.document_overrides, &self.overrides, own)
            || !same_set(&w.archived_objects, &self.archived, own)
        {
            return None;
        }
        let keys = w.requirements.keys().chain(self.requirements.keys());
        let mut ids: BTreeSet<&String> = keys
            .filter(|id| w.requirements.get(*id) != self.requirements.get(*id))
            .collect();
        // An object of a ledger type without an override takes the row of its requirement.
        if let Some(own) = self.own.as_deref().filter(|o| ledger_object(w, o)) {
            ids.insert(requirement_of(w, own)?);
        }
        let requirements = ids
            .iter()
            .filter_map(|id| Some(((*id).clone(), w.requirements.get(*id)?.clone())))
            .collect();
        let old: BTreeMap<String, Arc<Requirement>> = ids
            .iter()
            .filter_map(|id| Some(((*id).clone(), self.requirements.get(*id)?.clone())))
            .collect();
        let mut old_keys = BTreeSet::new();
        requirement_rows(w, &old, &mut |kind, id, _, _| {
            old_keys.insert(address(kind, id));
        });
        Some(Scope {
            requirements,
            old_keys,
        })
    }
}

/// True when the sync reads the object `own`: a present object of a ledger type without an
/// override.
fn ledger_object(w: &Workspace, own: &str) -> bool {
    let Some(o) = w.objects.get(own) else {
        return false;
    };
    let types = schema(w).types;
    let ledger = legacy_defaults().contains_key(&o.kind)
        || types
            .get(&o.kind)
            .is_some_and(|t| t.role == "ac" || t.role == "requirement");
    ledger && !w.document_overrides.contains(own)
}

/// The requirement whose rows hold `own`, or None when no requirement holds it.
fn requirement_of<'a>(w: &'a Workspace, own: &str) -> Option<&'a String> {
    w.requirements.iter().find_map(|(id, q)| {
        let r = q.revisions.last()?;
        let holds = requirement_address(id) == own
            || r.acs.iter().any(|ac| criterion_address(&ac.id) == own)
            || r.tasks
                .keys()
                .any(|t| address("task", &format!("{id}/{t}")) == own);
        holds.then_some(id)
    })
}
