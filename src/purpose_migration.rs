//! A Task has no purpose. The event migrate_purpose runs once in each workspace. It removes
//! `data.purpose` from each Task in a new revision that keeps the contract revision, so pins,
//! assessments and test results stay. A purpose of more than one word joins the Task text when
//! the text does not contain it. A one-word purpose was a label and adds nothing. A Task type
//! also loses `parent_decision`, the rule of the removed decision Tasks.
use crate::model::Workspace;
use crate::objects::{address, contract_of, schema};
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const REASON: &str = "A Task has no purpose";

/// The refusal of a purpose field on any node after the migration.
pub const REFUSAL: &str = "a node has no purpose field; read joppa_help";

/// The native document fields of a Task type after the migration have no purpose.
pub fn task_fields(w: &Workspace, fields: &'static [&'static str]) -> &'static [&'static str] {
    match fields {
        ["title", "owner", "body", "projects", "purpose"] if w.purpose_migrated => {
            &["title", "owner", "body", "projects"]
        }
        other => other,
    }
}

/// After the migration, a Task save has no purpose.
pub fn check_save(w: &Workspace, data: &Value) -> Result<()> {
    ensure!(
        !w.purpose_migrated || data.get("purpose").is_none(),
        REFUSAL
    );
    Ok(())
}

/// The refusal of the decision rule of a Task type after the migration.
pub const DECISION_REFUSAL: &str = "parent_decision is removed; read joppa_help";

/// After the migration, a Task type does not require a purpose and has no decision rule.
pub fn check_schema(
    w: &Workspace,
    types: &BTreeMap<String, crate::objects::TreeType>,
) -> Result<()> {
    if !w.purpose_migrated {
        return Ok(());
    }
    for (kind, t) in types {
        if t.role == "task" && t.required_fields.contains_key("purpose") {
            bail!("{kind}: {REFUSAL}");
        }
        if t.parent_decision {
            bail!("{kind}: {DECISION_REFUSAL}");
        }
    }
    Ok(())
}

/// Remove the purpose and join a purpose of more than one word to the text in `field`.
/// Gives true when the data changed.
pub fn drop_purpose(data: &mut Value, field: &str) -> bool {
    let Some(map) = data.as_object_mut() else {
        return false;
    };
    let Some(purpose) = map.remove("purpose") else {
        return false;
    };
    let purpose = purpose.as_str().unwrap_or_default().trim();
    let text = map.get(field).and_then(Value::as_str).unwrap_or_default();
    if purpose.contains(char::is_whitespace) && !text.contains(purpose) {
        let joined = if text.is_empty() {
            purpose.to_owned()
        } else {
            format!("{text}\n\n{purpose}")
        };
        map.insert(field.into(), json!(joined));
    }
    true
}

/// Apply migrate_purpose once. The workspace owner is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.purpose_migrated {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    let field = crate::body_migration::text_field(w, "contract");
    let mut next = schema(w);
    if next.types.values().any(|t| t.parent_decision) {
        for t in next.types.values_mut() {
            t.parent_decision = false;
        }
        next.revision += 1;
        next.actor = actor.into();
        next.at = at.into();
        next.position = position;
        next.reason = REASON.into();
        w.tree_schema.push(next);
    }
    let configured = schema(w);
    let task_role = |kind: &str| configured.types.get(kind).is_some_and(|t| t.role == "task");
    let mut texts = BTreeMap::new();
    for (key, o) in w.objects.iter_mut() {
        if !task_role(&o.kind) && !w.task_process.contains_key(key) {
            continue;
        }
        let mut revision = o.current().clone();
        if !drop_purpose(&mut revision.data, field) {
            continue;
        }
        let contract = contract_of(o.current());
        revision.revision += 1;
        revision.contract = Some([revision.revision, contract]);
        revision.schema_revision = configured.revision;
        revision.position = position;
        revision.actor = actor.into();
        revision.reason = REASON.into();
        revision.at = at.into();
        let text = revision.data[field].as_str().unwrap_or_default().to_owned();
        texts.insert(key.clone(), text);
        o.revisions.push(revision);
    }
    for (key, t) in w.task_process.iter_mut() {
        t.purpose.clear();
        if let Some(text) = texts.get(key) {
            t.contract = text.clone();
        }
    }
    for (id, q) in w.requirements.iter_mut() {
        let Some(r) = q.revisions.last_mut() else {
            continue;
        };
        for (tid, t) in r.tasks.iter_mut() {
            t.purpose.clear();
            if let Some(text) = texts.get(&address("task", &format!("{id}/{tid}"))) {
                t.contract = text.clone();
            }
        }
    }
    w.purpose_migrated = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_goes_and_a_sentence_joins_the_text_once() {
        let mut label = json!({"body":"Do it","purpose":"implementation"});
        assert!(drop_purpose(&mut label, "body"));
        assert_eq!(label, json!({"body":"Do it"}));
        let mut sentence = json!({"body":"Do it","purpose":"Prove the rule."});
        assert!(drop_purpose(&mut sentence, "body"));
        assert_eq!(sentence, json!({"body":"Do it\n\nProve the rule."}));
        let mut repeated = json!({"body":"Do it. Prove the rule.","purpose":"Prove the rule."});
        assert!(drop_purpose(&mut repeated, "body"));
        assert_eq!(repeated, json!({"body":"Do it. Prove the rule."}));
        let mut none = json!({"body":"Do it"});
        assert!(!drop_purpose(&mut none, "body"));
    }
}
