//! The text of every object type is `data.body`. The event migrate_body runs once in each
//! workspace. It moves `data.description` and `data.contract` into `data.body` in a new
//! revision that keeps the contract revision, so pins, assessments and test results stay.
//! It also renames these required fields of the tree schema and gives the typed records
//! the same text, so a later replay step derives the same documents.
use crate::model::Workspace;
use crate::objects::{address, contract_of, criterion_address, requirement_address, schema};
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The old text fields, in the order in which they join the body.
pub const OLD_FIELDS: [&str; 2] = ["description", "contract"];

pub const REASON: &str = "The text of every object type is body";

/// The refusal of an old text field after the migration.
pub fn refusal(field: &str) -> String {
    format!("use body instead of {field}; read joppa_help")
}

/// The data field of a typed text: `body` after the migration, else its old name.
pub fn text_field<'a>(w: &Workspace, old: &'a str) -> &'a str {
    if w.body_migrated { "body" } else { old }
}

/// The native document fields of a type after the migration: `body` for the old text fields.
pub fn body_fields(fields: &'static [&'static str]) -> &'static [&'static str] {
    match fields {
        ["title", "owner", "description", "archived"] => &["title", "owner", "body", "archived"],
        ["title", "owner", "description"] => &["title", "owner", "body"],
        [
            "title",
            "description",
            "owner",
            "decision_required",
            "depends_on",
            "before_launch",
        ] => &[
            "title",
            "body",
            "owner",
            "decision_required",
            "depends_on",
            "before_launch",
        ],
        ["title", "owner", "contract", "projects", "purpose"] => {
            &["title", "owner", "body", "projects", "purpose"]
        }
        other => other,
    }
}

/// After the migration, a save names its text `body`.
pub fn check_save(w: &Workspace, data: &Value) -> Result<()> {
    if w.body_migrated
        && let Some(field) = OLD_FIELDS.iter().find(|f| data.get(**f).is_some())
    {
        bail!(refusal(field));
    }
    Ok(())
}

/// Whether two revisions keep the title and the text. From the migration on, an old text
/// field reads as body, so the move to body changes no text; before it, each field counts.
pub fn same_text(w: &Workspace, before: &Value, after: &Value) -> bool {
    let (mut before, mut after) = (before.clone(), after.clone());
    if w.body_migrated {
        to_body(&mut before);
        to_body(&mut after);
    }
    crate::objects::TEXT_FIELDS
        .iter()
        .all(|field| before.get(*field) == after.get(*field))
}

/// After the migration, a type requires its text as `body`.
pub fn check_schema(
    w: &Workspace,
    types: &BTreeMap<String, crate::objects::TreeType>,
) -> Result<()> {
    if !w.body_migrated {
        return Ok(());
    }
    for (kind, t) in types {
        if let Some(field) = OLD_FIELDS
            .iter()
            .find(|f| t.required_fields.contains_key(**f))
        {
            bail!("{kind}: {}", refusal(field));
        }
    }
    Ok(())
}

/// Join the old text fields into `body` and remove them. A text that is empty or equal to
/// a part of the body adds nothing. Gives true when the data changed.
pub fn to_body(data: &mut Value) -> bool {
    let Some(map) = data.as_object_mut() else {
        return false;
    };
    let body = map.get("body").and_then(Value::as_str).unwrap_or_default();
    let mut parts: Vec<String> = vec![body.to_owned()];
    let mut changed = false;
    for field in OLD_FIELDS {
        let Some(other) = map.remove(field) else {
            continue;
        };
        changed = true;
        let other = other.as_str().unwrap_or_default();
        if !other.trim().is_empty() && !parts.iter().any(|p| p == other) {
            parts.push(other.to_owned());
        }
    }
    // An old field always leaves a body, also an empty one, as the typed records give it.
    if changed {
        let parts: Vec<&str> = parts
            .iter()
            .map(String::as_str)
            .filter(|p| !p.is_empty())
            .collect();
        map.insert("body".into(), json!(parts.join("\n\n")));
    }
    changed
}

/// Apply migrate_body once. The workspace owner is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.body_migrated {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    let mut next = schema(w);
    let mut schema_changed = false;
    for t in next.types.values_mut() {
        for field in OLD_FIELDS {
            if let Some(rule) = t.required_fields.remove(field) {
                t.required_fields.entry("body".into()).or_insert(rule);
                schema_changed = true;
            }
        }
    }
    if schema_changed {
        next.revision += 1;
        next.actor = actor.into();
        next.at = at.into();
        next.position = position;
        next.reason = REASON.into();
        w.tree_schema.push(next);
    }
    let schema_revision = schema(w).revision;
    let mut texts = BTreeMap::new();
    for (key, o) in w.objects.iter_mut() {
        let mut revision = o.current().clone();
        if !to_body(&mut revision.data) {
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
        let body = revision.data["body"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        texts.insert(key.clone(), body);
        o.revisions.push(revision);
    }
    typed_texts(w, &texts);
    w.body_migrated = true;
    Ok(())
}

/// Give each typed record the body of its object.
fn typed_texts(w: &mut Workspace, texts: &BTreeMap<String, String>) {
    let text = |key: String| texts.get(&key).cloned();
    for (id, d) in w.domains.iter_mut() {
        if let Some(body) = text(address("domain", id)) {
            d.description = body;
        }
    }
    for (id, c) in w.capabilities.iter_mut() {
        if let Some(body) = text(address("capability", id)) {
            c.description = body;
        }
    }
    for (id, c) in w.consumers.iter_mut() {
        if let Some(body) = text(address("consumer", id)) {
            c.description = body;
        }
    }
    for (id, q) in w.requirements.iter_mut() {
        let Some(r) = q.revisions.last_mut() else {
            continue;
        };
        if let Some(body) = text(requirement_address(id)) {
            r.body = body;
        }
        for ac in r.acs.iter_mut() {
            if let Some(body) = text(criterion_address(&ac.id)) {
                ac.description = body;
            }
        }
        for (tid, t) in r.tasks.iter_mut() {
            if let Some(body) = text(address("task", &format!("{id}/{tid}"))) {
                t.contract = body;
            }
        }
    }
    for (key, t) in w.task_process.iter_mut() {
        if let Some(body) = text(key.clone()) {
            t.contract = body;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_text_fields_join_the_body_once() {
        let mut ac = json!({"title":"A","description":"Given"});
        assert!(to_body(&mut ac));
        assert_eq!(ac, json!({"title":"A","body":"Given"}));
        let mut both = json!({"body":"Rule","description":"Why","contract":"Rule"});
        assert!(to_body(&mut both));
        assert_eq!(both, json!({"body":"Rule\n\nWhy"}));
        let mut empty = json!({"body":"Rule","contract":""});
        assert!(to_body(&mut empty));
        assert_eq!(empty, json!({"body":"Rule"}));
        let mut blank = json!({"title":"A","description":""});
        assert!(to_body(&mut blank));
        assert_eq!(blank, json!({"title":"A","body":""}));
        let mut plain = json!({"body":"Rule"});
        assert!(!to_body(&mut plain));
    }
}
