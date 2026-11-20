//! A human member redacts a secret from an object. Every revision in the current state shows
//! "[redacted]" in place of the fragment, so reads, history and search never return it.
//! The journal is append-only: the original stays there, and only the database owner reads it.
use crate::model::Workspace;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

pub const MASK: &str = "[redacted]";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Redaction {
    pub actor: String,
    pub reason: String,
    pub at: String,
    pub position: u64,
}

fn mask(value: &mut Value, fragment: &str) {
    match value {
        Value::String(s) => *s = s.replace(fragment, MASK),
        Value::Array(items) => items.iter_mut().for_each(|v| mask(v, fragment)),
        Value::Object(fields) => fields.values_mut().for_each(|v| mask(v, fragment)),
        _ => {}
    }
}

/// Mask `fragment` in each text of `entry`.
fn mask_entry<T: Serialize + DeserializeOwned>(entry: &mut T, fragment: &str) -> Result<()> {
    let mut value = serde_json::to_value(&*entry)?;
    mask(&mut value, fragment);
    *entry = serde_json::from_value(value)?;
    Ok(())
}

/// True when the JSON of `entry` holds `encoded`.
fn holds<T: Serialize>(entry: &T, encoded: &str) -> Result<bool> {
    Ok(serde_json::to_string(entry)?.contains(encoded))
}

pub fn redact(
    w: &mut Workspace,
    actor: &str,
    target: &str,
    fragment: &str,
    reason: &str,
    position: u64,
    at: &str,
) -> Result<()> {
    crate::commands::human_member(w, actor)?;
    crate::commands::nonempty(fragment)?;
    crate::commands::nonempty(reason)?;
    let key = crate::objects::resolve_address(w, target).to_owned();
    let object = std::sync::Arc::make_mut(w.objects.get_mut(&key).context("unknown object")?);
    // The state holds the loaded revisions; the store masks the stored past revisions too.
    for revision in &mut object.revisions {
        mask(&mut revision.data, fragment);
        revision.reason = revision.reason.replace(fragment, MASK);
    }
    // The requirements, with their revisions, comments and criteria, the intake records and the
    // notes of the object hold copies of the text too. Search reads them, so they mask it.
    let encoded = serde_json::to_string(fragment)?;
    let encoded = &encoded[1..encoded.len() - 1];
    for requirement in w.requirements.values_mut() {
        if holds(&**requirement, encoded)? {
            mask_entry(std::sync::Arc::make_mut(requirement), fragment)?;
        }
    }
    for intake in w.intake.values_mut() {
        if holds(intake, encoded)? {
            mask_entry(intake, fragment)?;
        }
    }
    for notes in w.notes.values_mut() {
        if holds(&**notes, encoded)? {
            mask_entry(std::sync::Arc::make_mut(notes), fragment)?;
        }
    }
    w.redactions.entry(key).or_default().push(Redaction {
        actor: actor.into(),
        reason: reason.into(),
        at: at.into(),
        position,
    });
    Ok(())
}
