//! The event migrate_generic_nodes runs once in each workspace: every node takes the same
//! fields and operations, whatever its type.
//! - The holder of each Task record becomes `data.assignee`, unless the document has one.
//! - The commit of a completed Task becomes `data.commit`.
//! - A done or retired Task is archived with the reason of its end.
//! - The priority of a node becomes `data.priority`.
//! - The Task records, the planning claims and the priorities leave the state.
use crate::model::Workspace;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const REASON: &str = "Every node takes the same fields and operations";

/// The changes of one node: the data fields to set and the reason of an archive.
#[derive(Default)]
struct Change {
    data: BTreeMap<&'static str, String>,
    end: Option<String>,
}

/// Apply migrate_generic_nodes once. The workspace creator is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.generic_nodes {
        return Ok(());
    }
    ensure!(
        actor == w.admin,
        "the workspace creator runs this migration"
    );
    let mut changes = BTreeMap::<String, Change>::new();
    for (key, t) in &w.task_process {
        let change = changes.entry(key.clone()).or_default();
        if let Some(holder) = crate::tasks::holder(w, t) {
            change.data.insert("assignee", holder.into());
        }
        if let Some(commit) = &t.commit {
            change.data.insert("commit", commit.clone());
        }
        change.end = match (&t.retired, &t.done) {
            (Some(reason), _) => Some(format!("Task retired: {reason}")),
            (None, Some(evidence)) => Some(format!("Task done: {evidence}")),
            (None, None) => None,
        };
    }
    for (key, priority) in &w.priorities {
        let key = crate::objects::resolve_address(w, key).to_owned();
        changes
            .entry(key)
            .or_default()
            .data
            .insert("priority", priority.clone());
    }
    let schema_revision = crate::objects::schema(w).revision;
    for (key, change) in changes {
        let Some(o) = w.objects.get_mut(&key).filter(|o| o.current().present) else {
            continue;
        };
        let mut next = o.current().clone();
        for (field, value) in change.data {
            if next.data.get(field).is_none_or(Value::is_null) {
                next.data[field] = json!(value);
            }
        }
        next.present = change.end.is_none();
        if next.data == o.current().data && next.present {
            continue;
        }
        next.reason = change.end.unwrap_or_else(|| REASON.into());
        next.revision += 1;
        next.schema_revision = schema_revision;
        next.position = position;
        next.actor = actor.into();
        next.at = at.into();
        let archived = !next.present;
        o.revisions.push(next);
        if archived {
            w.archived_objects.insert(key);
        }
    }
    w.task_process.clear();
    w.task_context.clear();
    w.planning_claims.clear();
    w.priorities.clear();
    w.generic_nodes = true;
    Ok(())
}
