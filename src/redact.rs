//! The owner redacts a secret from an object. Every revision in the current state shows
//! "[redacted]" in place of the fragment, so reads, history and search never return it.
//! The journal is append-only: the original stays there, and only the database owner reads it.
use crate::model::Workspace;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
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

pub fn redact(
    w: &mut Workspace,
    actor: &str,
    target: &str,
    fragment: &str,
    reason: &str,
    position: u64,
    at: &str,
) -> Result<()> {
    ensure!(actor == w.admin, "workspace owner required");
    crate::commands::nonempty(fragment)?;
    crate::commands::nonempty(reason)?;
    let key = crate::objects::resolve_address(w, target).to_owned();
    let object = w.objects.get_mut(&key).context("unknown object")?;
    // The state holds the loaded revisions; the store masks the stored past revisions too.
    for revision in &mut object.revisions {
        mask(&mut revision.data, fragment);
        revision.reason = revision.reason.replace(fragment, MASK);
    }
    w.redactions.entry(key).or_default().push(Redaction {
        actor: actor.into(),
        reason: reason.into(),
        at: at.into(),
        position,
    });
    Ok(())
}
