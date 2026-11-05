//! An open question on a node is its readiness mark: `assess` with `ready: false` and the
//! question as reason. The event migrate_decision_required runs once in each workspace. It
//! removes `data.decision_required` from each object in a new revision that keeps the contract
//! revision, so pins, assessments and test results stay. A criterion record keeps no flag.
use crate::commands::{Command, SaveObjectOp, SetTreeSchemaOp};
use crate::model::Workspace;
use crate::objects::{contract_of, schema};
use anyhow::{Result, ensure};

/// The removed document field.
pub const FIELD: &str = "decision_required";

pub const REASON: &str = "An open question is a readiness mark";

/// The refusal of the removed field in a live save or in a type rule.
pub const REFUSAL: &str = "decision_required was removed; ask an open question with assess {target, ready: false, reason}";

/// After the migration, a live save and a type rule have no decision_required. The replay
/// still reads the older saves.
pub fn check(w: &Workspace, command: &Command) -> Result<()> {
    let used = match command {
        Command::SaveObject(SaveObjectOp { data, .. }) => data.get(FIELD).is_some(),
        Command::SetTreeSchema(SetTreeSchemaOp {
            types, put_types, ..
        }) => types
            .iter()
            .chain(put_types)
            .flat_map(|m| m.values())
            .any(|t| t.required_fields.contains_key(FIELD)),
        _ => false,
    };
    ensure!(!(w.decision_required_migrated && used), REFUSAL);
    Ok(())
}

/// Apply migrate_decision_required once. The workspace owner is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.decision_required_migrated {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    let schema_revision = schema(w).revision;
    for o in w.objects.values_mut() {
        let mut revision = o.current().clone();
        let removed = revision.data.as_object_mut().and_then(|m| m.remove(FIELD));
        if removed.is_none() {
            continue;
        }
        let contract = contract_of(o.current());
        revision.revision += 1;
        revision.contract = Some([revision.revision, contract]);
        revision.schema_revision = schema_revision;
        revision.position = position;
        revision.actor = actor.into();
        revision.reason = REASON.into();
        revision.at = at.into();
        o.revisions.push(revision);
    }
    // A criterion owner that equals the requirement owner is the inherited owner, as a save
    // without the flag gives it.
    for q in w.requirements.values_mut() {
        let Some(r) = q.revisions.last_mut() else {
            continue;
        };
        for ac in r.acs.iter_mut() {
            ac.decision_required = None;
            ac.owner = ac.owner.take().filter(|owner| *owner != r.owner);
        }
    }
    w.decision_required_migrated = true;
    Ok(())
}
