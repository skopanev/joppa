//! The event migrate_decision_required runs once in each workspace. It removes
//! `data.decision_required` from each object in a new revision that keeps the contract
//! revision, so pins and test results stay. A stored criterion record of a
//! requirement is immutable, so it keeps its old flag, and no read gives that flag.
use crate::commands::{Command, SaveObjectOp, SetTreeSchemaOp};
use crate::model::Workspace;
use crate::objects::{contract_of, schema};
use anyhow::{Result, ensure};

/// The removed document field.
pub const FIELD: &str = "decision_required";

/// The reason of each migration revision. The replay writes the same text, so it stays.
pub const REASON: &str = "An open question is a readiness mark";

/// The refusal of the removed field in a live save or in a type rule.
pub const REFUSAL: &str = "decision_required was removed; ask an open question with note";

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
    w.decision_required_migrated = true;
    Ok(())
}
