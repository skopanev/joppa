//! Part of `objects`: `parent_valid` and the items after it, moved from objects.rs unchanged.
use super::*;

pub(crate) fn parent_valid(
    types: &BTreeMap<String, TreeType>,
    w: &Workspace,
    kind: &str,
    parent: Option<&str>,
    child: Option<&str>,
) -> Result<()> {
    let t = types
        .get(kind)
        .context("object type is absent from the tree schema")?;
    match parent {
        None => ensure!(t.root, "this type requires a parent"),
        Some(p) => {
            let obj = w
                .objects
                .get(resolve_address(w, p))
                .context("unknown parent")?;
            ensure!(
                obj.current().present
                    || !w.schema_migrated && kind == "task" && obj.kind == "ac"
                    || child.is_some_and(|key| w.task_process.contains_key(key)
                        && w.objects
                            .get(key)
                            .is_some_and(|o| o.current().parent.as_deref() == Some(p))),
                "parent is absent from the current tree"
            );
            ensure!(
                t.parents.iter().any(|v| v == "*" || v == &obj.kind),
                "parent type is not permitted by the tree schema"
            );
        }
    }
    Ok(())
}

/// A document value fails a required field rule. HTTP returns 422.
#[derive(Debug)]
pub struct RequiredField(pub String);

impl std::fmt::Display for RequiredField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RequiredField {}

/// The document fields that hold the title and the text of an object.
pub(crate) const TEXT_FIELDS: [&str; 4] = ["title", "body", "description", "contract"];

/// Validate current writes without applying new gates to journal replay.
pub fn validate_authoring_questions(w: &Workspace, command: &Command, position: u64) -> Result<()> {
    if matches!(
        command,
        Command::SaveObject(crate::commands::SaveObjectOp { .. })
    ) {
        return Ok(());
    }
    let types = schema(w).types;
    for o in w
        .objects
        .values()
        .filter(|o| o.current().present && o.current().position == position)
    {
        if types.get(&o.kind).is_none_or(|t| t.question.is_none()) {
            continue;
        }
        let previous = o.revisions.iter().rev().find(|r| r.position < position);
        // Inbox decisions skip this gate only when the title and description stay unchanged.
        let changed = TEXT_FIELDS
            .iter()
            .any(|field| previous.and_then(|r| r.data.get(field)) != o.current().data.get(field));
        if changed {
            return Err(RequiredField(
                "This type has a question. Use save_object with question_checked".into(),
            )
            .into());
        }
    }
    Ok(())
}

pub(crate) fn validate_required_fields(t: &TreeType, data: &Value) -> Result<()> {
    for (field, rule) in &t.required_fields {
        let valid = match rule {
            RequiredFieldRule::Presence(_) => match data.get(field) {
                None | Some(Value::Null) => false,
                Some(Value::String(value)) => !value.trim().is_empty(),
                Some(Value::Array(value)) => !value.is_empty(),
                Some(Value::Object(value)) => !value.is_empty(),
                Some(_) => true,
            },
            RequiredFieldRule::Text(rule) => data
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(|text| rule.contains.iter().all(|marker| text.contains(marker))),
        };
        if !valid {
            return Err(RequiredField(rule.error().to_owned()).into());
        }
    }
    Ok(())
}

/// The status rules of a type. The rule tests of a criterion type stays valid only for the
/// replay of the events before migrate_test_nodes.
pub const STATUS_RULES: [&str; 4] = ["own", "children", "links", "none"];

/// The status rule of a type. A type without a rule has the rule unset: its nodes take
/// the tone orange.
pub fn status_rule(t: &TreeType) -> &str {
    t.status.as_deref().unwrap_or("unset")
}

/// The status rule that the role of a type gave before migrate_test_rules. Only the
/// migration and the replay of older events use it.
pub fn role_status_rule(t: &TreeType) -> &str {
    t.status.as_deref().unwrap_or(match t.role.as_str() {
        "ac" => "tests",
        "group" | "requirement" => "children",
        _ => "none",
    })
}

/// The reasons of the schema revisions that the migrations write. A replay of an old
/// migration event writes the same revision, so these texts never change.
pub(crate) const NOTES_SCHEMA_REASON: &str = "Replace incoming source types with Note";

pub(crate) const REFERENCES_SCHEMA_REASON: &str =
    "Record configurable reference and presentation rules";

pub(crate) const ROLES_SCHEMA_REASON: &str = "Use configured roles and flags for every object type";

pub const TASK_GATE_SCHEMA_REASON: &str = "Record the default Task readiness gate";

/// The relations that a link between two objects can have.
pub const LINK_RELATIONS: [&str; 2] = ["related", "depends_on"];

#[allow(clippy::too_many_arguments)]
/// The priority values of the workspace, the first value first in work order.
pub fn priority_values(w: &Workspace) -> Vec<String> {
    schema(w)
        .priorities
        .unwrap_or_else(|| ["p0", "p1", "p2"].map(String::from).to_vec())
}

#[allow(clippy::too_many_arguments)]
/// Any member of the workspace edits the tree: the schema, the order, archives and restores.
pub(crate) fn may_edit_tree(w: &Workspace, actor: &str) -> bool {
    actor == w.admin || w.members.contains_key(actor)
}
