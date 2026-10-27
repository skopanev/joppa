//! The migrate_consumers event kept one consumer on each criterion. A criterion whose
//! old list named more valid consumers lost the others without a trace on the board.
//! The migrate_consumer_drops event runs once after it. It writes one comment on each such
//! criterion and lists the consumers that the migration removed, so the owner can add a
//! criterion for each of them. The comment changes no revision and no status. The
//! earlier revisions keep the complete old list.
use crate::consumer_migration::{FIELD, REASON, convert_types, listed, valid};
use crate::model::{Note, Workspace};
use crate::objects::{ObjectRevision, annotation_address, resolve_address, schema};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::collections::BTreeMap;

/// The identifier of the comment on each criterion.
pub const COMMENT: &str = "consumer-migration";

/// The valid consumers of `ids`, in their order, each once.
fn valid_once(w: &Workspace, ids: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if valid(w, &id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// The latest revision of `address` at or before `position`.
fn at_position<'a>(w: &'a Workspace, address: &str, position: u64) -> Option<&'a ObjectRevision> {
    w.objects
        .get(resolve_address(w, address))?
        .revisions
        .iter()
        .rev()
        .find(|r| r.position <= position)
}

/// The text of the comment.
pub fn text(kept: &str, removed: &[String]) -> String {
    format!(
        "The consumer migration kept one consumer on this criterion: {kept}. \
         It removed these consumers from the criterion: {}. \
         If the rule must also hold for them, add one criterion for each of them.",
        removed.join(", ")
    )
}

/// The removed consumers of each live criterion, by its address.
pub fn removed(w: &Workspace) -> BTreeMap<String, (String, Vec<String>)> {
    let mut out = BTreeMap::new();
    let Some(index) = w.tree_schema.iter().position(|s| s.reason == REASON) else {
        return out;
    };
    let migration = &w.tree_schema[index];
    let mut types = match index {
        0 => crate::objects::schema_migration_defaults(),
        _ => w.tree_schema[index - 1].types.clone(),
    };
    let old = convert_types(&mut types);
    let criteria: Vec<String> = schema(w)
        .types
        .into_iter()
        .filter(|(kind, t)| t.role == "ac" && old.contains_key(kind))
        .map(|(kind, _)| kind)
        .collect();
    for (key, o) in &w.objects {
        if !criteria.contains(&o.kind) || !o.current().present {
            continue;
        }
        let Some(step) = o
            .revisions
            .iter()
            .position(|r| r.reason == REASON && r.position == migration.position)
        else {
            continue;
        };
        let (Some(kept), Some(before)) = (
            o.revisions[step].data.get(FIELD).and_then(Value::as_str),
            step.checked_sub(1).map(|i| &o.revisions[i]),
        ) else {
            continue;
        };
        let mut list = valid_once(w, listed(&before.data, &old[&o.kind]));
        if list.is_empty()
            && let Some(parent) = before.parent.as_deref()
            && let Some(p) = at_position(w, parent, migration.position)
        {
            let kind = w.objects[resolve_address(w, parent)].kind.clone();
            let fields = old.get(&kind).cloned().unwrap_or_default();
            list = valid_once(w, listed(&p.data, &fields));
        }
        list.retain(|id| id != kept);
        if !list.is_empty() {
            out.insert(key.clone(), (kept.to_owned(), list));
        }
    }
    out
}

/// Apply migrate_consumer_drops once. The workspace owner is the actor of the event.
pub fn apply(w: &mut Workspace, actor: &str, at: &str) -> Result<()> {
    if w.consumer_drops_noted {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    w.consumer_drops_noted = true;
    for (key, (kept, list)) in removed(w) {
        let address = annotation_address(w, &key);
        let notes = w.notes.entry(address).or_default();
        if notes.comments.contains_key(COMMENT) {
            continue;
        }
        notes.comments.insert(
            COMMENT.into(),
            Note {
                actor: actor.into(),
                at: at.into(),
                text: text(&kept, &list),
                reply_to: None,
                withdrawn: false,
            },
        );
    }
    Ok(())
}
