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

/// The status rules of a type and the tones that a children rule can order.
pub const STATUS_RULES: [&str; 4] = ["tests", "children", "links", "none"];

pub const STATUS_TONES: [&str; 3] = ["red", "grey", "green"];

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

/// The types of a schema with each children order filled from the schema status list.
pub fn ordered_types(s: &SchemaRevision) -> BTreeMap<String, TreeType> {
    let list = crate::status_colors::effective(s.statuses.as_ref(), s.status_colors.as_ref());
    let order = crate::status_colors::children_order(&list);
    let mut types = s.types.clone();
    for t in types.values_mut() {
        t.status_order.get_or_insert_with(|| order.clone());
    }
    types
}

/// The order of a children rule: its own order, else red, grey, green.
pub fn status_order(t: &TreeType) -> Vec<String> {
    t.status_order
        .clone()
        .unwrap_or_else(|| STATUS_TONES.iter().map(|s| (*s).to_owned()).collect())
}

/// The reasons of the schema revisions that the migrations write. A replay of an old
/// migration event writes the same revision, so these texts never change.
pub(crate) const NOTES_SCHEMA_REASON: &str = "Replace incoming source types with Note";

pub(crate) const REFERENCES_SCHEMA_REASON: &str =
    "Record configurable reference and presentation rules";

pub(crate) const ROLES_SCHEMA_REASON: &str = "Use configured roles and flags for every object type";

pub const TASK_GATE_SCHEMA_REASON: &str = "Record the default Task readiness gate";

/// The workspace uses the built-in tree: every schema revision comes from a migration.
pub(crate) fn builtin_tree(w: &Workspace) -> bool {
    w.tree_schema.iter().all(|r| {
        [
            NOTES_SCHEMA_REASON,
            TASK_GATE_SCHEMA_REASON,
            REFERENCES_SCHEMA_REASON,
            ROLES_SCHEMA_REASON,
            crate::consumer_migration::REASON,
        ]
        .contains(&r.reason.as_str())
    })
}

/// The process fields that objects of a type show. A saved schema lists them per type. The
/// built-in tree, until the workspace saves its own schema, shows confirmed and acceptance on
/// a requirement and confirmed on a criterion. The defaults stay out of `defaults()`, because
/// an old schema migration event replays from those types.
pub fn process_fields(w: &Workspace, kind: &str) -> Vec<String> {
    let s = schema(w);
    let Some(t) = s.types.get(kind) else {
        return vec![];
    };
    if let Some(fields) = &t.process_fields {
        return fields.clone();
    }
    match (builtin_tree(w), t.role.as_str()) {
        (true, "requirement") => vec!["confirmed".into(), "acceptance".into()],
        (true, "ac") => vec!["confirmed".into()],
        _ => vec![],
    }
}

/// The relations that a link between two objects can have.
pub const LINK_RELATIONS: [&str; 2] = ["related", "depends_on"];

#[allow(clippy::too_many_arguments)]
/// The priority values of the workspace, the first value first in work order.
pub fn priority_values(w: &Workspace) -> Vec<String> {
    schema(w)
        .priorities
        .unwrap_or_else(|| ["p0", "p1", "p2"].map(String::from).to_vec())
}

/// The priority of a node: its own value of the current list, else the last value.
pub fn priority(w: &Workspace, key: &str) -> String {
    let values = priority_values(w);
    w.priorities
        .get(resolve_address(w, key))
        .filter(|v| values.contains(v))
        .cloned()
        .unwrap_or_else(|| values.last().cloned().unwrap_or_default())
}

/// The work order key of a node: the rank of each priority from the root down to it.
pub fn priority_chain(w: &Workspace, key: &str) -> Vec<usize> {
    let values = priority_values(w);
    let mut chain = vec![];
    let mut seen = BTreeSet::new();
    let mut current = Some(resolve_address(w, key).to_owned());
    while let Some(node) = current.filter(|n| seen.insert(n.clone())) {
        let value = priority(w, &node);
        chain.push(
            values
                .iter()
                .position(|v| *v == value)
                .unwrap_or(values.len()),
        );
        current = w
            .objects
            .get(&node)
            .and_then(|o| o.current().parent.clone())
            .map(|p| resolve_address(w, &p).to_owned());
    }
    chain.reverse();
    chain
}

#[allow(clippy::too_many_arguments)]
/// Any member of the workspace edits the tree: the schema, the order, archives and restores.
pub(crate) fn may_edit_tree(w: &Workspace, actor: &str) -> bool {
    actor == w.admin || w.members.contains_key(actor)
}
