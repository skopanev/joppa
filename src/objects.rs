//! Common document revisions derived from the immutable workspace journal.
use crate::{
    commands::{self, Command},
    model::*,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TreeType {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub root: bool,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_gate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_blocker: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_preview_length: Option<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub references: BTreeMap<String, ReferenceRule>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_fields: BTreeMap<String, RequiredFieldRule>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parent_decision: bool,
    /// How a node of this type gets its tone: tests, children, links or none. Without a value the
    /// role decides: the criterion role uses tests, group and requirement use children.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The order in which a children or links rule takes the first status that it finds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_order: Option<Vec<String>>,
    /// The process fields that an object of this type shows: confirmed for a requirement or a
    /// criterion, acceptance for a requirement. A type without the list shows neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_fields: Option<Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RequiredFieldRule {
    Presence(String),
    Text(TextFieldRule),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextFieldRule {
    pub contains: Vec<String>,
    pub error: String,
}
impl RequiredFieldRule {
    fn error(&self) -> &str {
        match self {
            Self::Presence(error) => error,
            Self::Text(rule) => &rule.error,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRule {
    pub target: String,
    pub title: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub verification_subject: bool,
    /// The reference also takes the explicit value all: every object of the target type.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_all: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaRevision {
    pub revision: u32,
    pub types: BTreeMap<String, TreeType>,
    /// The ordered priority values, the first value first in work order. Without a saved
    /// list the values are p0, p1 and p2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priorities: Option<Vec<String>>,
    /// The colour of each status dot, as #RRGGBB per tone. Without it the board colours stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_colors: Option<BTreeMap<String, String>>,
    /// The ordered status tones with a light and a dark colour; a higher entry wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<crate::status_colors::Status>>,
    /// The test rules: the block test_results, see result_shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_results: Option<crate::result_shape::ResultShape>,
    /// With true, set_tree_schema refuses a type without a status rule. With false, it saves
    /// the type, and its nodes take the tone of an unset rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_required: Option<bool>,
    pub actor: String,
    pub at: String,
    pub position: u64,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObjectLink {
    pub target: String,
    pub relation: String,
    #[serde(default)]
    pub revision: Option<u32>,
    #[serde(default)]
    pub blocking: bool,
}
/// An ordering dependency stores no revision field. Other links keep their stored null,
/// so the documents saved before ordering links serialize as they were stored.
impl Serialize for ObjectLink {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let unpinned = self.relation == "depends_on" && self.revision.is_none();
        let mut link = serializer.serialize_struct("ObjectLink", 4)?;
        link.serialize_field("target", &self.target)?;
        link.serialize_field("relation", &self.relation)?;
        if unpinned {
            link.skip_field("revision")?;
        } else {
            link.serialize_field("revision", &self.revision)?;
        }
        link.serialize_field("blocking", &self.blocking)?;
        link.end()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectRevision {
    pub revision: u32,
    pub schema_revision: u32,
    pub parent: Option<String>,
    pub data: Value,
    pub links: Vec<ObjectLink>,
    pub actor: String,
    pub at: String,
    pub position: u64,
    pub reason: String,
    pub present: bool,
    /// When this revision changes only the test list: [this revision, the revision of the
    /// contract]. Pins and Task reviews compare the contract, so a new test list asks for no
    /// review. A revision that copies this one has another number, so the pair stops counting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<[u32; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Object {
    pub kind: String,
    pub id: String,
    pub revisions: Vec<ObjectRevision>,
    #[serde(default)]
    pub parent_override: bool,
}
impl Object {
    pub fn current(&self) -> &ObjectRevision {
        self.revisions.last().expect("object has a revision")
    }
}
/// The built-in tree of a new workspace: each criterion names one consumer.
pub fn defaults() -> BTreeMap<String, TreeType> {
    let mut types = schema_migration_defaults();
    crate::consumer_migration::convert_types(&mut types);
    // The built-in tree names the status rule of each type.
    for t in types.values_mut() {
        t.status = Some(role_status_rule(t).to_owned());
    }
    types
}
/// The built-in tree as the migrate_schema event saw it, with the consumers of a
/// requirement. An old journal replays from these types, so they never change.
pub fn schema_migration_defaults() -> BTreeMap<String, TreeType> {
    [
        ("domain", "Domain", "group", true, vec![]),
        ("capability", "Capability", "group", false, vec!["domain"]),
        (
            "requirement",
            "Requirement",
            "requirement",
            false,
            vec!["capability"],
        ),
        (
            "ac",
            "Acceptance criterion",
            "ac",
            false,
            vec!["requirement"],
        ),
        ("task", "Task", "task", false, vec!["ac"]),
        ("consumer", "Consumer", "document", true, vec![]),
        ("note", "Note", "document", true, vec!["*"]),
        ("project", "Project", "document", true, vec![]),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (kind, title, role, root, parents))| {
        (
            kind.into(),
            TreeType {
                question: None,
                index: Some(index as u32),
                references: BTreeMap::new(),
                required_fields: BTreeMap::new(),
                parent_decision: false,
                status: None,
                status_order: None,
                process_fields: None,
                color: None,
                title_size: None,
                badge: None,
                title_preview_length: None,
                parent_gate: (kind == "task").then(|| "ready".into()),
                launch_blocker: (kind == "ac").then_some(true),
                inbox: (kind == "note").then_some(true),
                title: title.into(),
                role: role.into(),
                root,
                parents: parents.into_iter().map(String::from).collect(),
            },
        )
    })
    .collect::<BTreeMap<String, TreeType>>()
    .into_iter()
    .map(|(kind, mut t)| {
        let color = match kind.as_str() {
            "domain" => "#17685f",
            "capability" => "#6551a1",
            "consumer" => "#247eab",
            "note" => "#7664aa",
            _ => "#63756b",
        };
        t.color = Some(color.into());
        if kind == "requirement" {
            t.references.insert(
                "consumers".into(),
                ReferenceRule {
                    target: "consumer".into(),
                    title: "Used by".into(),
                    required: true,
                    multiple: true,
                    verification_subject: false,
                    allow_all: false,
                },
            );
        }
        if kind == "task" {
            t.references.insert(
                "projects".into(),
                ReferenceRule {
                    target: "project".into(),
                    title: "Repository".into(),
                    required: false,
                    multiple: true,
                    verification_subject: true,
                    allow_all: false,
                },
            );
            t.parent_decision = true;
        }
        (kind, t)
    })
    .collect()
}
pub fn legacy_defaults() -> BTreeMap<String, TreeType> {
    let mut types = schema_migration_defaults();
    for t in types.values_mut() {
        t.index = None;
        t.parent_gate = None;
        t.color = None;
        t.references.clear();
        t.parent_decision = false;
        t.inbox = None;
        t.launch_blocker = None;
    }
    for (kind, title, root, parents) in [
        ("intake", "Source note", true, vec![]),
        ("point", "Source point", false, vec!["intake".into()]),
    ] {
        types.insert(
            kind.into(),
            TreeType {
                question: None,
                index: None,
                references: BTreeMap::new(),
                required_fields: BTreeMap::new(),
                parent_decision: false,
                status: None,
                status_order: None,
                process_fields: None,
                inbox: None,
                launch_blocker: None,
                color: None,
                title_size: None,
                badge: None,
                title_preview_length: None,
                parent_gate: None,
                title: title.into(),
                role: "document".into(),
                root,
                parents,
            },
        );
    }
    types
}
pub fn schema(w: &Workspace) -> SchemaRevision {
    w.tree_schema
        .last()
        .cloned()
        .unwrap_or_else(|| SchemaRevision {
            revision: 0,
            types: legacy_defaults(),
            priorities: None,
            status_colors: None,
            statuses: None,
            test_results: None,
            status_required: None,
            actor: w.admin.clone(),
            at: String::new(),
            position: 0,
            reason: "Default workspace tree".into(),
        })
}
pub fn migrate_schema(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.schema_migrated {
        return Ok(());
    }
    let mut current = schema(w);
    let preset = schema_migration_defaults();
    let kinds: BTreeSet<_> = current.types.keys().cloned().collect();
    for (kind, t) in &mut current.types {
        if let Some(default) = preset.get(kind) {
            if t.references.is_empty() {
                t.references = default
                    .references
                    .iter()
                    .filter(|(_, r)| kinds.contains(&r.target))
                    .map(|(field, r)| (field.clone(), r.clone()))
                    .collect();
            }
            if t.color.is_none() {
                t.color = default.color.clone();
            }
            if t.role == default.role
                && t.parents == default.parents
                && t.parent_gate == default.parent_gate
            {
                t.parent_decision = default.parent_decision;
            }
        }
    }
    current.revision += 1;
    current.actor = actor.into();
    current.at = at.into();
    current.position = position;
    current.reason = REFERENCES_SCHEMA_REASON.into();
    w.tree_schema.push(current);
    w.schema_migrated = true;
    Ok(())
}
/// Activate schema-driven writes without changing any saved document revision.
pub fn migrate_documents(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.documents_migrated {
        return Ok(());
    }
    ensure!(
        w.schema_migrated && w.tasks_migrated,
        "migrate schema and tasks first"
    );
    let mut configured = schema(w);
    for (kind, t) in &mut configured.types {
        t.inbox = Some(t.inbox.unwrap_or(kind == "note"));
        t.launch_blocker = Some(t.launch_blocker.unwrap_or(kind == "ac"));
    }
    configured.revision += 1;
    configured.actor = actor.into();
    configured.at = at.into();
    configured.position = position;
    configured.reason = ROLES_SCHEMA_REASON.into();
    w.tree_schema.push(configured);
    w.documents_migrated = true;
    Ok(())
}

/// The reference targets of a document for a write: a required reference must be there.
pub fn reference_targets(
    w: &Workspace,
    kind: &str,
    data: &Value,
) -> Result<Vec<(String, String, bool)>> {
    targets_of(w, kind, data, true)
}
/// The reference targets of a stored document for a read. A document from before a
/// required reference existed still gives its other targets.
pub fn stored_reference_targets(
    w: &Workspace,
    kind: &str,
    data: &Value,
) -> Result<Vec<(String, String, bool)>> {
    targets_of(w, kind, data, false)
}
fn targets_of(
    w: &Workspace,
    kind: &str,
    data: &Value,
    enforce_required: bool,
) -> Result<Vec<(String, String, bool)>> {
    let t = schema(w)
        .types
        .get(kind)
        .cloned()
        .context("unknown object type")?;
    let mut targets = vec![];
    for (field, rule) in &t.references {
        // The explicit value all names every object of the target type.
        if rule.allow_all && data.get(field).and_then(Value::as_str) == Some("all") {
            continue;
        }
        let values: Vec<&str> = match data.get(field) {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(values)) if rule.multiple => values
                .iter()
                .map(|v| v.as_str().context("reference IDs must be strings"))
                .collect::<Result<_>>()?,
            Some(Value::String(value)) if !rule.multiple => vec![value],
            _ => bail!("reference {field} has invalid cardinality"),
        };
        ensure!(
            !enforce_required || !rule.required || !values.is_empty(),
            "reference {field} is required"
        );
        let mut seen = BTreeSet::new();
        for value in values {
            let target = if value.contains(':') {
                value.into()
            } else {
                ensure!(
                    rule.target != "*",
                    "a wildcard reference needs a full object address"
                );
                address(&rule.target, value)
            };
            let target = resolve_address(w, &target).to_owned();
            let object = w
                .objects
                .get(&target)
                .filter(|o| o.current().present)
                .context(format!("unknown reference target: {target}"))?;
            ensure!(
                rule.target == "*" || rule.target == object.kind,
                "reference {field} targets the wrong object type"
            );
            ensure!(
                seen.insert(target.clone()),
                "duplicate reference in {field}"
            );
            targets.push((field.clone(), target, rule.verification_subject));
        }
    }
    Ok(targets)
}
pub fn referenced(w: &Workspace, key: &str, target: &str) -> bool {
    let target = resolve_address(w, target);
    let mut current = Some(key);
    let mut seen = BTreeSet::new();
    while let Some(key) = current {
        let key = resolve_address(w, key);
        if !seen.insert(key) {
            return false;
        }
        let Some(o) = w.objects.get(key) else {
            return false;
        };
        if stored_reference_targets(w, &o.kind, &o.current().data)
            .is_ok_and(|refs| refs.iter().any(|(_, id, _)| id == target))
        {
            return true;
        }
        current = o.current().parent.as_deref();
    }
    false
}
fn address(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}
pub fn resolve_address<'a>(w: &'a Workspace, key: &'a str) -> &'a str {
    w.object_aliases.get(key).map(String::as_str).unwrap_or(key)
}

pub fn original_identity<'a>(w: &'a Workspace, kind: &'a str, id: &'a str) -> (&'a str, &'a str) {
    let key = address(kind, id);
    w.object_aliases
        .iter()
        .find(|(_, target)| *target == &key)
        .and_then(|(source, _)| source.split_once(':'))
        .unwrap_or((kind, id))
}

pub fn migrate_notes(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    if w.notes_migrated {
        return Ok(());
    }
    let mut candidate = w.clone();
    let previous_schema = schema(w);
    let reason = NOTES_SCHEMA_REASON;
    let sources: Vec<_> = w
        .objects
        .iter()
        .filter(|(_, o)| o.current().present && ["intake", "point"].contains(&o.kind.as_str()))
        .map(|(key, o)| (key.clone(), o.clone()))
        .collect();
    for (key, _) in &sources {
        let target = format!("note:source-{:x}", Sha256::digest(key.as_bytes()));
        ensure!(
            !w.objects.contains_key(&target),
            "Note migration address collision"
        );
        candidate.object_aliases.insert(key.clone(), target);
    }
    let mut types = previous_schema.types;
    types.remove("intake");
    types.remove("point");
    for t in types.values_mut() {
        for parent in &mut t.parents {
            if ["intake", "point"].contains(&parent.as_str()) {
                *parent = "note".into();
            }
        }
        let mut seen = BTreeSet::new();
        t.parents.retain(|parent| seen.insert(parent.clone()));
    }
    let schema_revision = previous_schema.revision + 1;
    candidate.tree_schema.push(SchemaRevision {
        revision: schema_revision,
        types,
        priorities: previous_schema.priorities.clone(),
        status_colors: previous_schema.status_colors.clone(),
        statuses: previous_schema.statuses.clone(),
        test_results: previous_schema.test_results.clone(),
        status_required: previous_schema.status_required,
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.into(),
    });
    for (key, old) in sources {
        let target = candidate.object_aliases[&key].clone();
        let mut current = old.current().clone();
        current.revision += 1;
        current.schema_revision = schema_revision;
        current.actor = actor.into();
        current.at = at.into();
        current.position = position;
        current.reason = reason.into();
        current.parent = current
            .parent
            .as_deref()
            .map(|parent| resolve_address(&candidate, parent).to_owned());
        if old.kind == "point" {
            current.data["body"] = current.data["title"].clone();
            let source = old.id.split_once('/').context("invalid source point")?.0;
            if let Some(intake) = w.intake.get(source) {
                current.data["author"] = json!(intake.author);
            }
            let kind = current.data["kind"].as_str().unwrap_or("");
            current.data["review_policy"] = json!(w.questions.get(kind));
            if let Some(req) = current.data["requirement"].as_str() {
                let link = ObjectLink {
                    target: requirement_address(req),
                    relation: "related".into(),
                    revision: None,
                    blocking: false,
                };
                if !current
                    .links
                    .iter()
                    .any(|l| l.target == link.target && l.relation == link.relation)
                {
                    current.links.push(link);
                }
            }
        } else {
            current.data["review_policy"] = json!(w.questions);
        }
        let mut note = old.clone();
        note.kind = "note".into();
        note.id = target.strip_prefix("note:").unwrap().into();
        note.parent_override = true;
        note.revisions.push(current);
        candidate.objects.insert(target.clone(), note);
        let mut retired = old.current().clone();
        retired.revision += 1;
        retired.present = false;
        retired.actor = actor.into();
        retired.at = at.into();
        retired.position = position;
        retired.reason = reason.into();
        candidate
            .objects
            .get_mut(&key)
            .unwrap()
            .revisions
            .push(retired);
        if let Some(notes) = candidate.notes.remove(&key) {
            candidate.notes.insert(target, notes);
        }
    }
    candidate.intake.clear();
    candidate.questions.clear();
    candidate.notes_migrated = true;
    validate_tree(&candidate)?;
    *w = candidate;
    Ok(())
}
fn parent_valid(
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
const TEXT_FIELDS: [&str; 4] = ["title", "body", "description", "contract"];

/// Validate current writes without applying new gates to journal replay.
pub fn validate_authoring_questions(w: &Workspace, command: &Command, position: u64) -> Result<()> {
    if matches!(command, Command::SaveObject { .. }) {
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

fn validate_required_fields(t: &TreeType, data: &Value) -> Result<()> {
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
const NOTES_SCHEMA_REASON: &str = "Replace incoming source types with Note";
const REFERENCES_SCHEMA_REASON: &str = "Record configurable reference and presentation rules";
const ROLES_SCHEMA_REASON: &str = "Use configured roles and flags for every object type";
pub const TASK_GATE_SCHEMA_REASON: &str = "Record the default Task readiness gate";

/// The workspace uses the built-in tree: every schema revision comes from a migration.
fn builtin_tree(w: &Workspace) -> bool {
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

#[allow(clippy::too_many_arguments)]
pub fn set_schema(
    w: &mut Workspace,
    actor: &str,
    revision: u32,
    types: &BTreeMap<String, TreeType>,
    priorities: Option<&Vec<String>>,
    status_colors: Option<&BTreeMap<String, String>>,
    statuses: Option<&Vec<crate::status_colors::Status>>,
    test_results: Option<&Value>,
    status_required: Option<bool>,
    reason: &str,
    position: u64,
    at: &str,
) -> Result<()> {
    ensure!(may_edit_tree(w, actor), "workspace member required");
    ensure!(
        schema(w).revision == revision,
        "stale schema revision; read the tree schema again"
    );
    commands::nonempty(reason)?;
    ensure!(
        !types.is_empty() && types.len() <= 100,
        "tree schema needs 1 to 100 types"
    );
    if let Some(values) = priorities {
        let mut seen = BTreeSet::new();
        ensure!(
            (1..=20).contains(&values.len())
                && values.iter().all(|v| (1..=32).contains(&v.chars().count())
                    && v.chars().all(|c| !c.is_whitespace() && !c.is_control())
                    && seen.insert(v.as_str())),
            "a priority list has 1 to 20 different values without spaces"
        );
    }
    if let Some(colors) = status_colors {
        crate::status_colors::validate(colors)?;
    }
    if let Some(list) = statuses {
        crate::status_colors::validate_statuses(list)?;
    }
    let test_results = test_results
        .map(|raw| crate::test_rules::parse_block(w, raw))
        .transpose()?;
    for field in test_results.iter().flat_map(|s| &s.fields) {
        if let Some(target) = &field.target {
            ensure!(
                types.contains_key(target),
                "ref field {} targets the unknown type {target}",
                field.name
            );
        }
    }
    for (kind, t) in types {
        commands::identifier(kind)?;
        commands::nonempty(&t.title)?;
        ensure!(
            t.color.as_deref().is_none_or(|color| color.len() == 7
                && color.starts_with('#')
                && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)),
            "type color must use #RRGGBB"
        );
        ensure!(
            t.title_size.is_none_or(|size| (12..=48).contains(&size)),
            "type title_size must be an integer from 12 to 48 pixels"
        );
        ensure!(
            t.badge
                .as_deref()
                .is_none_or(|badge| (1..=8).contains(&badge.chars().count())
                    && badge.chars().all(|c| !c.is_whitespace() && !c.is_control())),
            "type badge must contain 1 to 8 characters without spaces or control characters"
        );
        ensure!(
            t.title_preview_length
                .is_none_or(|length| (1..=256).contains(&length)),
            "type title_preview_length must be an integer from 1 to 256 characters"
        );
        ensure!(
            t.status
                .as_deref()
                .is_none_or(|rule| STATUS_RULES.contains(&rule)),
            "a status rule is tests, children, links or none"
        );
        ensure!(
            t.process_fields
                .as_ref()
                .is_none_or(|fields| fields.iter().all(|f| {
                    (f == "confirmed" && matches!(t.role.as_str(), "requirement" | "ac"))
                        || (f == "acceptance" && t.role == "requirement")
                })),
            "process_fields allows confirmed for a requirement or a criterion and acceptance for a requirement"
        );
        ensure!(
            t.status_order.as_ref().is_none_or(|order| !order.is_empty()
                && order.iter().all(|s| STATUS_TONES.contains(&s.as_str()))),
            "a status order allows only red, grey and green"
        );
        ensure!(
            t.parent_gate
                .as_deref()
                .is_none_or(|g| ["ready", "verified", "completed"].contains(&g)),
            "unknown parent gate"
        );
        ensure!(
            t.parent_gate.is_none() || t.role == "task",
            "parent gates apply to Task roles"
        );
        ensure!(
            ["group", "document", "requirement", "ac", "task"].contains(&t.role.as_str()),
            "unknown semantic role"
        );
        ensure!(
            !t.parent_decision || t.role == "task",
            "parent decisions apply to Task roles"
        );
        for (field, rule) in &t.references {
            commands::identifier(field)?;
            commands::nonempty(&rule.title)?;
            ensure!(
                rule.target == "*" || types.contains_key(&rule.target),
                "unknown reference target type"
            );
            ensure!(
                ![
                    "title",
                    "owner",
                    "body",
                    "description",
                    "contract",
                    "purpose",
                    "status",
                    "archived",
                    "decision_required",
                    "depends_on",
                    "before_launch"
                ]
                .contains(&field.as_str()),
                "reference field conflicts with a document field"
            );
        }
        for (field, rule) in &t.required_fields {
            commands::identifier(field)?;
            commands::nonempty(rule.error())?;
            if let RequiredFieldRule::Text(rule) = rule {
                ensure!(
                    !rule.contains.is_empty(),
                    "a text rule needs at least one marker"
                );
                for marker in &rule.contains {
                    commands::nonempty(marker)?;
                }
            }
            ensure!(
                !process_field(field),
                "a required field must be a document field"
            );
        }
        for p in &t.parents {
            ensure!(p == "*" || types.contains_key(p), "unknown parent type");
        }
    }
    for obj in w.objects.values().filter(|o| o.current().present) {
        ensure!(
            types.contains_key(&obj.kind),
            "object type is absent from the tree schema"
        );
        validate_required_fields(&types[&obj.kind], &obj.current().data)?;
        if w.task_process.contains_key(&address(&obj.kind, &obj.id)) {
            ensure!(
                types[&obj.kind].role == "task",
                "a type with existing Tasks must retain its Task role"
            );
        }
        let key = address(&obj.kind, &obj.id);
        if requirement_key(w, &key).is_some() || criterion_context(w, &key).is_some() {
            ensure!(
                types[&obj.kind].role == schema(w).types[&obj.kind].role,
                "a type with existing verification evidence must retain its role"
            );
        }
        if !legacy_task_binding(w, obj) {
            parent_valid(
                types,
                w,
                &obj.kind,
                obj.current().parent.as_deref(),
                Some(&address(&obj.kind, &obj.id)),
            )?;
        }
    }
    let priorities = priorities.cloned().or_else(|| schema(w).priorities);
    let status_colors = status_colors.cloned().or_else(|| schema(w).status_colors);
    let statuses = statuses.cloned().or_else(|| schema(w).statuses);
    let test_results = test_results.or_else(|| schema(w).test_results);
    let status_required = status_required.or_else(|| schema(w).status_required);
    if status_required == Some(true) {
        for (kind, t) in types {
            ensure!(
                t.status.is_some(),
                "type {kind} needs a status rule: tests, children, links or none"
            );
        }
    }
    w.tree_schema.push(SchemaRevision {
        revision: revision + 1,
        types: types.clone(),
        status_required,
        priorities,
        status_colors,
        statuses,
        test_results,
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.into(),
    });
    Ok(())
}

/// Preserve local journal keys while new types use complete object addresses.
pub fn requirement_address(key: &str) -> String {
    if key.contains(':') {
        key.into()
    } else {
        address("requirement", key)
    }
}
pub fn criterion_address(key: &str) -> String {
    if key.contains(':') {
        key.into()
    } else {
        address("ac", key)
    }
}
fn process_key(w: &Workspace, kind: &str, id: &str, legacy_kind: &str) -> String {
    let key = address(kind, id);
    if w.documents_migrated {
        if legacy_kind == "requirement" {
            if let Some(existing) = requirement_key(w, &key) {
                return existing.into();
            }
        } else if let Some((_, _, criterion)) = criterion_record_context(w, &key) {
            return criterion.id.clone();
        }
        return key;
    }
    if kind == legacy_kind { id.into() } else { key }
}
fn requirement_key<'a>(w: &'a Workspace, key: &str) -> Option<&'a str> {
    let key = resolve_address(w, key);
    let obj = w.objects.get(key)?;
    w.requirements
        .get_key_value(key)
        .or_else(|| {
            (obj.kind == "requirement")
                .then(|| w.requirements.get_key_value(&obj.id))
                .flatten()
        })
        .map(|(key, _)| key.as_str())
}

fn document_adapter_fields(w: &Workspace, kind: &str) -> Option<&'static [&'static str]> {
    let configured = schema(w);
    let role = configured.types.get(kind).map(|t| t.role.as_str());
    if w.documents_migrated && role == Some("requirement") {
        return Some(&["title", "body", "owner"]);
    }
    native_document_fields(match role {
        Some(role @ ("requirement" | "ac")) => role,
        _ => kind,
    })
}

fn document_rows(w: &Workspace) -> BTreeMap<String, (String, Option<String>, Value)> {
    let mut rows = BTreeMap::new();
    let mut put = |kind: &str, id: &str, parent: Option<String>, data: Value| {
        rows.insert(address(kind, id), (kind.into(), parent, data));
    };
    for (id, d) in &w.domains {
        put(
            "domain",
            id,
            None,
            json!({"title":d.title,"owner":d.owner,"description":d.description,"archived":d.archived}),
        );
    }
    for (id, c) in &w.capabilities {
        put(
            "capability",
            id,
            c.domain.as_deref().map(|id| address("domain", id)),
            json!({"title":c.title,"owner":c.owner,"description":c.description}),
        );
    }
    for (id, c) in &w.consumers {
        put(
            "consumer",
            id,
            None,
            json!({"title":c.title,"owner":c.owner,"description":c.description}),
        );
    }
    for (id, p) in &w.projects {
        put(
            "project",
            id,
            None,
            json!({"title":id,"repository":p,"owner":w.admin}),
        );
    }
    for (id, q) in &w.requirements {
        if let Some(r) = q.revisions.last() {
            let req_address = requirement_address(id);
            let (req_kind, req_id) = req_address.split_once(':').unwrap();
            let mut data =
                json!({"title":r.title,"body":r.body,"owner":r.owner,"consumers":r.consumers});
            if w.documents_migrated && r.consumers.is_empty() {
                data.as_object_mut().unwrap().remove("consumers");
            }
            put(
                req_kind,
                req_id,
                q.capability.as_deref().map(|id| address("capability", id)),
                data,
            );
            for ac in &r.acs {
                let ac_address = criterion_address(&ac.id);
                let (ac_kind, ac_id) = ac_address.split_once(':').unwrap();
                put(
                    ac_kind,
                    ac_id,
                    Some(req_address.clone()),
                    json!({"title":ac.text,"description":ac.description,"owner":ac.owner.as_ref().unwrap_or(&r.owner),"decision_required":ac.owner.is_some(),"depends_on":ac.depends_on,"before_launch":ac.before_launch}),
                );
            }
            for (tid, t) in &r.tasks {
                let mut data = json!({"title":t.title,"owner":r.owner,"contract":t.contract,"projects":t.projects,"purpose":t.purpose});
                let parent = if t.acs.len() == 1 {
                    criterion_address(&t.acs[0])
                } else {
                    // Preserve old bindings without permitting new Tasks with multiple ACs.
                    data["legacy_acs"] = json!(t.acs);
                    requirement_address(id)
                };
                put("task", &format!("{id}/{tid}"), Some(parent), data);
            }
        }
    }
    for (id, i) in &w.intake {
        put(
            "intake",
            id,
            None,
            json!({"title":id,"body":i.said,"owner":i.owner.as_ref().unwrap_or(&w.admin),"author":i.author}),
        );
        for (pid, p) in &i.points {
            put(
                "point",
                &format!("{id}/{pid}"),
                Some(address("intake", id)),
                json!({"title":p.said,"owner":i.owner.as_ref().unwrap_or(&w.admin),"kind":p.kind,"answers":p.answers,"outcome":p.outcome,"reason":p.reason,"requirement":p.requirement}),
            );
        }
    }
    rows
}
/// Replay old commands into the same document ledger. Process evidence stays in its original revision.
pub fn sync(w: &mut Workspace, actor: &str, command: &Command, position: u64, at: &str) {
    sync_with(w, actor, command, position, at, true, None);
}

/// `sync` that can leave unchanged sibling criteria of a revised specification alone.
/// With `force_siblings`, every criterion of the specification gets a new revision.
/// `spec_reason` replaces the reason of the new specification revision.
fn sync_with(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
    force_siblings: bool,
    spec_reason: Option<&str>,
) {
    let rows = document_rows(w);
    let reason = serde_json::to_value(command)
        .ok()
        .and_then(|v| v["reason"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "Journal operation".into());
    let configured = schema(w);
    let schema_revision = configured.revision;
    let adapter_fields: BTreeMap<_, _> = configured
        .types
        .keys()
        .map(|kind| (kind.clone(), document_adapter_fields(w, kind)))
        .collect();
    let forced_req = match command {
        Command::Revise { req, .. } => Some(req.as_str()),
        _ => None,
    };
    for (key, (kind, parent, data)) in &rows {
        if w.task_process.contains_key(key)
            || w.document_overrides.contains(key)
            || w.archived_objects.contains(key)
        {
            continue;
        }
        let obj = w.objects.entry(key.clone()).or_insert_with(|| Object {
            kind: kind.clone(),
            id: key.split_once(':').unwrap().1.into(),
            revisions: vec![],
            parent_override: false,
        });
        if matches!(command,Command::Revise {req,capability,..} if key==&requirement_address(req) && capability.as_ref().is_some_and(|cap|obj.revisions.last().is_some_and(|r|r.parent.as_deref()!=Some(&address("capability",cap)))))
        {
            obj.parent_override = false;
        }
        // The migrated specification owns its parent through both sync passes.
        // Only an explicit legacy grouping change can replace that parent here.
        let preserve_document_parent = w.documents_migrated
            && !obj.revisions.is_empty()
            && configured
                .types
                .get(kind)
                .is_some_and(|t| t.role == "requirement")
            && !matches!(command, Command::Revise { req, capability: Some(_), .. } if key == &requirement_address(req));
        let parent = if obj.parent_override || preserve_document_parent {
            obj.revisions.last().and_then(|v| v.parent.clone())
        } else {
            parent.clone()
        };
        let forced = forced_req.is_some_and(|req| {
            key == &requirement_address(req)
                || (force_siblings
                    && w.requirements
                        .get(req)
                        .and_then(|q| q.revisions.last())
                        .is_some_and(|r| r.acs.iter().any(|ac| criterion_address(&ac.id) == *key)))
        });
        let mut data = data.clone();
        if w.schema_migrated
            && let Some(previous) = obj.revisions.last()
            && let Some(t) = w.tree_schema.last().and_then(|s| s.types.get(kind))
        {
            for (field, value) in previous.data.as_object().unwrap() {
                let legacy_reference_update = field == "consumers"
                    && matches!(command, Command::Revise { req, consumers, .. } if key == &requirement_address(req) && w.documents_migrated && !consumers.is_empty());
                if !legacy_reference_update
                    && adapter_fields
                        .get(kind)
                        .copied()
                        .flatten()
                        .is_none_or(|fields| !fields.contains(&field.as_str()))
                {
                    data[field] = value.clone();
                }
            }
            for field in t.references.keys() {
                let native_update = field == "consumers"
                    && matches!(command, Command::Revise { req, consumers, .. } if key == &requirement_address(req) && (!w.documents_migrated || !consumers.is_empty()));
                if native_update && !t.references[field].multiple {
                    data[field] = data[field]
                        .as_array()
                        .and_then(|values| values.first())
                        .cloned()
                        .unwrap_or(Value::Null);
                } else if !native_update && let Some(value) = previous.data.get(field) {
                    data[field] = value.clone();
                }
            }
        }
        if w.schema_migrated
            && !w.documents_migrated
            && let Some(previous) = obj.revisions.last()
            && let Some(t) = w.tree_schema.last().and_then(|s| s.types.get(kind))
        {
            for field in schema_migration_defaults()
                .get(kind)
                .into_iter()
                .flat_map(|t| t.references.keys())
            {
                if !t.references.contains_key(field) && previous.data.get(field).is_none() {
                    data.as_object_mut().unwrap().remove(field);
                }
            }
        }
        // Only a new criterion takes a consumer here; a stored one keeps its document.
        if obj.revisions.is_empty()
            && let Some(t) = configured.types.get(kind)
        {
            let requirement = parent.as_ref().and_then(|p| rows.get(p)).map(|(_, _, d)| d);
            crate::consumer_migration::fill(t, &mut data, requirement);
        }
        let changed = obj
            .revisions
            .last()
            .is_none_or(|v| v.data != data || v.parent != parent || !v.present)
            || forced;
        if changed {
            let links = obj
                .revisions
                .last()
                .map(|v| v.links.clone())
                .unwrap_or_default();
            let spec = forced_req.is_some_and(|req| key == &requirement_address(req));
            obj.revisions.push(ObjectRevision {
                revision: obj.revisions.last().map_or(1, |r| r.revision + 1),
                schema_revision,
                parent,
                data: data.clone(),
                links,
                actor: actor.into(),
                at: at.into(),
                position,
                reason: spec_reason
                    .filter(|_| spec)
                    .map_or_else(|| reason.clone(), str::to_owned),
                present: true,
                contract: None,
            });
        }
    }
    let roles = schema(w).types;
    for (key, obj) in &mut w.objects {
        if (legacy_defaults().contains_key(&obj.kind)
            || roles
                .get(&obj.kind)
                .is_some_and(|t| t.role == "ac" || t.role == "requirement"))
            && obj.kind != "note"
            && !w.task_process.contains_key(key)
            && !w.document_overrides.contains(key)
            && !rows.contains_key(key)
            && obj.current().present
        {
            let mut r = obj.current().clone();
            r.revision += 1;
            r.present = false;
            r.actor = actor.into();
            r.at = at.into();
            r.position = position;
            r.reason = reason.clone();
            obj.revisions.push(r);
        }
    }
}

/// `kept` holds the links of the current revision: one of them can keep an absent target.
fn validate_links(
    w: &Workspace,
    key: &str,
    links: &[ObjectLink],
    kept: &[ObjectLink],
) -> Result<()> {
    let mut unique = BTreeSet::new();
    // A Task dependency pins its target contract; another node orders work without a pin.
    let task = key
        .split_once(':')
        .is_some_and(|(kind, _)| task_kind(w, kind));
    for link in links {
        ensure!(
            LINK_RELATIONS.contains(&link.relation.as_str()),
            "unknown link relation"
        );
        ensure!(
            unique.insert((&link.target, &link.relation)),
            "duplicate object link"
        );
        let target = w
            .objects
            .get(resolve_address(w, &link.target))
            .context("unknown linked object in this workspace")?;
        ensure!(
            target.current().present || kept.contains(link),
            "linked object is absent from the current tree"
        );
        if let Some(r) = link.revision {
            ensure!(
                r > 0 && r <= target.current().revision,
                "unknown linked revision"
            );
        }
        if task && link.relation == "depends_on" {
            ensure!(
                link.revision.is_some(),
                "dependency needs a target revision"
            );
        }
        if link.blocking {
            ensure!(
                link.relation == "depends_on",
                "blocking links must be dependencies"
            );
        }
    }
    fn visit(
        w: &Workspace,
        key: &str,
        changed: &str,
        links: &[ObjectLink],
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        let key = resolve_address(w, key);
        if done.contains(key) {
            return Ok(());
        }
        ensure!(stack.insert(key.into()), "dependency cycle");
        let current = if key == changed {
            links
        } else {
            &w.objects
                .get(key)
                .context("unknown dependency")?
                .current()
                .links
        };
        for link in current.iter().filter(|l| l.blocking) {
            visit(w, &link.target, changed, links, stack, done)?;
        }
        if let Some(parent) = w
            .objects
            .get(key)
            .and_then(|o| o.current().parent.as_deref())
        {
            visit(w, parent, changed, links, stack, done)?;
        }
        stack.remove(key);
        done.insert(key.into());
        Ok(())
    }
    visit(
        w,
        key,
        key,
        links,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )
}
fn parent_cycle(w: &Workspace, key: &str, parent: Option<&str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    seen.insert(key.to_string());
    let mut p = parent;
    while let Some(id) = p {
        let id = resolve_address(w, id);
        ensure!(seen.insert(id.into()), "tree parent cycle");
        p = w
            .objects
            .get(resolve_address(w, id))
            .and_then(|o| o.current().parent.as_deref());
    }
    Ok(())
}
fn string(data: &Value, name: &str) -> Result<String> {
    data[name]
        .as_str()
        .map(str::to_owned)
        .with_context(|| format!("missing text field {name}"))
}
fn optional_string(data: &Value, name: &str) -> Result<Option<String>> {
    match data.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(
            v.as_str()
                .with_context(|| format!("{name} must be text"))?
                .into(),
        )),
    }
}
fn list(data: &Value, name: &str) -> Result<Vec<String>> {
    serde_json::from_value(data.get(name).cloned().unwrap_or_else(|| json!([])))
        .with_context(|| format!("{name} must contain identifiers"))
}
fn local(parent: Option<&str>, kind: &str) -> Result<String> {
    parent
        .and_then(|v| v.strip_prefix(&format!("{kind}:")))
        .map(str::to_owned)
        .with_context(|| format!("parent must be {kind}:id"))
}
fn fresh_acs(r: &Revision) -> Vec<Ac> {
    r.acs
        .iter()
        .cloned()
        .map(|mut a| {
            a.assessment = None;
            a
        })
        .collect()
}
/// The adapter writes a whole requirement revision, which starts without assessments.
/// Each criterion whose record did not change keeps its readiness; `reset_changed`
/// clears the criteria below a changed title, text, link or parent.
fn keep_assessments(w: &mut Workspace, req: &str) {
    let Some([.., previous, current]) = w
        .requirements
        .get_mut(req)
        .map(|q| q.revisions.as_mut_slice())
    else {
        return;
    };
    commands::carry_assessments(&previous.acs, &mut current.acs);
}
fn revise(
    req: &str,
    r: &Revision,
    capability: Option<String>,
    data: &Value,
    reason: &str,
    acs: Vec<Ac>,
) -> Result<Command> {
    Ok(Command::Revise {
        req: req.into(),
        expected_revision: r.number,
        capability,
        consumers: list(data, "consumers")?,
        title: string(data, "title")?,
        body: string(data, "body")?,
        owner: string(data, "owner")?,
        reason: reason.into(),
        acs,
        coverage_policy: true,
        carry_tasks: true,
    })
}
pub(crate) fn native_document_fields(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "domain" => Some(&["title", "owner", "description", "archived"]),
        "capability" | "consumer" => Some(&["title", "owner", "description"]),
        "requirement" => Some(&["title", "body", "owner", "consumers"]),
        "ac" => Some(&[
            "title",
            "description",
            "owner",
            "decision_required",
            "depends_on",
            "before_launch",
        ]),
        "task" => Some(&["title", "owner", "contract", "projects", "purpose"]),
        _ => None,
    }
}

pub(crate) fn process_field(field: &str) -> bool {
    matches!(
        field,
        "confirmed"
            | "assessment"
            | "acceptance"
            | "accepted"
            | "runs"
            | "acs"
            | "tasks"
            | "assignee"
            | "claim"
            | "planning_claim"
            | "done"
            | "retired"
            | "blocked"
            | "review_required"
            | "verification"
            | "work_ready"
            | "evidence"
    )
}

fn typed_command(
    w: &Workspace,
    kind: &str,
    id: &str,
    parent: Option<&str>,
    data: &Value,
    reason: &str,
) -> Result<Option<Command>> {
    let configured = schema(w);
    let role = configured
        .types
        .get(kind)
        .context("unknown object type")?
        .role
        .as_str();
    let workflow = if w.documents_migrated {
        if !matches!(role, "requirement" | "ac") {
            return Ok(None);
        }
        role
    } else if matches!(role, "requirement" | "ac") {
        role
    } else if matches!(kind, "requirement" | "ac") {
        return Ok(None);
    } else {
        kind
    };
    let mut native_data = data.clone();
    if w.documents_migrated && workflow == "requirement" {
        native_data["consumers"] = json!([]);
    }
    if w.schema_migrated
        && !w.documents_migrated
        && workflow == "requirement"
        && let Some(value) = data.get("consumers").and_then(Value::as_str)
    {
        native_data["consumers"] = json!([value]);
    }
    let data = &native_data;
    let exists = w.objects.contains_key(&address(kind, id));
    let allowed = native_document_fields(workflow);
    if let Some(allowed) = allowed {
        for field in data.as_object().context("data must be an object")?.keys() {
            ensure!(
                allowed.contains(&field.as_str()) || w.schema_migrated && !process_field(field),
                "unknown document field {field}; use process commands for evidence"
            );
        }
    }
    let description =
        || Ok::<_, anyhow::Error>(optional_string(data, "description")?.unwrap_or_default());
    let cmd = match workflow {
        "domain" => {
            ensure!(
                data.get("archived").is_none_or(|v| Some(v)
                    == w.objects
                        .get(&address(kind, id))
                        .map(|o| &o.current().data["archived"])
                    || !exists && v == false),
                "use set_domain_archived to change archive state"
            );
            if exists {
                Command::EditDomain {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                }
            } else {
                Command::Domain {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                }
            }
        }
        "capability" => {
            if exists {
                Command::EditCapability {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: optional_string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                }
            } else {
                Command::Capability {
                    id: id.into(),
                    title: string(data, "title")?,
                    domain: ancestor(w, parent, "domain").ok(),
                    owner: optional_string(data, "owner")?,
                    description: description()?,
                }
            }
        }
        "consumer" => {
            if exists {
                Command::EditConsumer {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                }
            } else {
                Command::Consumer {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                }
            }
        }
        "requirement" => {
            let consumers = list(data, "consumers")?;
            if !w.schema_migrated {
                ensure!(!consumers.is_empty(), "REQ needs at least one Consumer");
            }
            let capability = if w.documents_migrated {
                None
            } else {
                ancestor(w, parent, "capability").ok()
            };
            let req_key = process_key(w, kind, id, "requirement");
            if let Some(q) = w.requirements.get(&req_key) {
                let r = q.revisions.last().unwrap();
                revise(&req_key, r, capability, &native_data, reason, fresh_acs(r))?
            } else {
                Command::Revise {
                    req: req_key,
                    expected_revision: 0,
                    capability,
                    consumers,
                    title: string(data, "title")?,
                    body: string(data, "body")?,
                    owner: string(data, "owner")?,
                    reason: reason.into(),
                    acs: vec![],
                    coverage_policy: true,
                    carry_tasks: true,
                }
            }
        }
        "ac" => {
            let Some((req, _)) = parent.and_then(|p| verification_context(w, p)) else {
                return Ok(None);
            };
            let req = req.to_owned();
            let q = w.requirements.get(&req).context("unknown parent REQ")?;
            let r = q.revisions.last().unwrap();
            if exists {
                ensure!(
                    criterion_context(w, &address(kind, id))
                        .is_none_or(|(old_req, _, _)| old_req == req),
                    "criterion cannot move to another verification contract"
                );
            }
            let assigned = optional_string(data, "owner")?.unwrap_or_else(|| r.owner.clone());
            commands::human(w, &assigned)?;
            let decision = data
                .get("decision_required")
                .map(|v| v.as_bool().context("decision_required must be boolean"))
                .transpose()?
                .unwrap_or(false);
            ensure!(
                decision || assigned == r.owner,
                "an independent AC owner requires decision_required=true"
            );
            let ac_key = process_key(w, kind, id, "ac");
            let ac = Ac {
                id: ac_key.clone(),
                text: string(data, "title")?,
                description: description()?,
                owner: decision.then_some(assigned),
                depends_on: list(data, "depends_on")?,
                before_launch: data
                    .get("before_launch")
                    .map(|v| v.as_bool().context("before_launch must be boolean"))
                    .transpose()?
                    .unwrap_or(false),
                assessment: None,
            };
            let mut acs = fresh_acs(r);
            if let Some(old) = acs.iter_mut().find(|a| a.id == ac_key) {
                *old = ac;
            } else {
                acs.push(ac);
            }
            let mut req_data = w.objects[&requirement_address(&req)].current().data.clone();
            if w.schema_migrated
                && let Some(value) = req_data
                    .get("consumers")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            {
                req_data["consumers"] = json!([value]);
            }
            let capability = (!w.documents_migrated)
                .then(|| q.capability.clone())
                .flatten();
            if w.documents_migrated {
                req_data["consumers"] = json!([]);
            }
            revise(&req, r, capability, &req_data, reason, acs)?
        }
        "task" => {
            let ac_id = local(parent, "ac")?;
            let ac = w
                .objects
                .get(&address("ac", &ac_id))
                .context("unknown AC")?;
            let req = local(ac.current().parent.as_deref(), "requirement")?;
            let r = w.requirements[&req].revisions.last().unwrap();
            ensure!(
                optional_string(data, "owner")?.is_none_or(|owner| owner == r.owner),
                "Task inherits its REQ owner"
            );
            let tid = id
                .strip_prefix(&format!("{req}/"))
                .context("task id must be <REQ>/<task>")?;
            if exists {
                let old = w.objects[&address(kind, id)].current();
                ensure!(
                    old.parent == parent.map(str::to_owned),
                    "Task cannot move to another AC"
                );
                ensure!(
                    old.data["purpose"] == data["purpose"],
                    "task purpose cannot change"
                );
                Command::AmendTask {
                    req,
                    revision: r.number,
                    id: tid.into(),
                    title: string(data, "title")?,
                    contract: string(data, "contract")?,
                    projects: list(data, "projects")?,
                    reason: reason.into(),
                }
            } else {
                Command::Task {
                    req,
                    revision: r.number,
                    id: tid.into(),
                    title: string(data, "title")?,
                    contract: string(data, "contract")?,
                    projects: list(data, "projects")?,
                    purpose: string(data, "purpose")?,
                    acs: vec![ac_id],
                    assignee: None,
                }
            }
        }
        "project" | "intake" | "point" => return Ok(None),
        _ => return Ok(None),
    };
    Ok(Some(cmd))
}
fn ancestor(w: &Workspace, parent: Option<&str>, kind: &str) -> Result<String> {
    let mut p = parent;
    let mut seen = BTreeSet::new();
    while let Some(key) = p {
        ensure!(seen.insert(key), "tree parent cycle");
        let o = w
            .objects
            .get(resolve_address(w, key))
            .context("unknown ancestor")?;
        if o.kind == kind {
            return Ok(o.id.clone());
        }
        p = o.current().parent.as_deref();
    }
    bail!("semantic {kind} ancestor is required")
}
pub fn save(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<()> {
    if let Some(mut candidate) = saved(w, actor, command, position, at, false)? {
        mark_contract(w, &mut candidate, command);
        *w = candidate;
    }
    Ok(())
}

/// A new revision that changes only the test list keeps the contract revision of the
/// revision before it.
fn mark_contract(w: &Workspace, candidate: &mut Workspace, command: &Command) {
    let Command::SaveObject { kind, id, .. } = command else {
        return;
    };
    let key = address(kind, id);
    let Some(old) = w.objects.get(resolve_address(w, &key)).map(Object::current) else {
        return;
    };
    let target = resolve_address(candidate, &key).to_owned();
    let Some(new) = candidate
        .objects
        .get_mut(&target)
        .and_then(|o| o.revisions.last_mut())
    else {
        return;
    };
    if new.revision != old.revision && tests_only_change(old, new) {
        new.contract = Some([new.revision, contract_of(old)]);
    }
}
/// A live save whose normalized data, parent and links equal the current revision changes nothing.
pub fn unchanged_save(
    w: &Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<bool> {
    Ok(saved(w, actor, command, position, at, true)?.is_none())
}
fn saved(
    w: &Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
    accept_unchanged: bool,
) -> Result<Option<Workspace>> {
    let Command::SaveObject {
        id,
        kind,
        revision,
        parent,
        data,
        links,
        reason,
        ..
    } = command
    else {
        bail!("save_object required")
    };
    let parent = &parent
        .as_deref()
        .map(|key| resolve_address(w, key).to_owned());
    commands::nonempty(reason)?;
    ensure!(
        !id.is_empty()
            && id.len() <= 200
            && !id.contains(':')
            && !id.chars().any(char::is_whitespace),
        "invalid object id"
    );
    commands::identifier(kind)?;
    let key = address(kind, id);
    ensure!(
        !w.documents_migrated || !w.object_aliases.contains_key(&key),
        "object address is a historical alias"
    );
    let old = w.objects.get(&key).cloned();
    ensure!(
        old.as_ref().map_or(0, |o| o.current().revision) == *revision,
        "stale object revision; read the object again"
    );
    if let Some(old) = &old {
        ensure!(
            old.current().present,
            "object is absent from the current tree"
        );
    }
    ensure!(data.is_object(), "data must be an object");
    ensure!(
        data.get("verification").is_none(),
        "verification comes from test reports"
    );
    if w.documents_migrated {
        for field in data.as_object().unwrap().keys() {
            ensure!(!process_field(field), "use process commands for {field}");
        }
    }
    if input_type(w, kind) {
        ensure!(
            data.get("status")
                .is_none_or(|v| matches!(v.as_str(), Some("unprocessed" | "processed"))),
            "Note status must be unprocessed or processed"
        );
    }
    commands::nonempty(&string(data, "title")?)?;
    validate_required_fields(
        schema(w).types.get(kind).context("unknown object type")?,
        data,
    )?;
    if w.test_rules_migrated {
        crate::ac_tests::validate_field(w, kind, data)?;
    }
    parent_valid(&schema(w).types, w, kind, parent.as_deref(), Some(&key))?;
    parent_cycle(w, &key, parent.as_deref())?;
    if w.schema_migrated {
        reference_targets(w, kind, data)?;
    }
    let kept = old
        .as_ref()
        .map(|o| o.current().links.clone())
        .unwrap_or_default();
    validate_links(w, &key, links, &kept)?;
    let unchanged = old
        .as_ref()
        .is_some_and(|o| o.current().data == *data && o.current().parent == *parent);
    let same = |saved: &Value| {
        accept_unchanged
            && old.as_ref().is_some_and(|o| {
                let r = o.current();
                r.data == *saved && r.parent == *parent && r.links == *links
            })
    };
    let mut candidate = w.clone();
    let generic_task = schema(w).types[kind].role == "task" && w.tasks_migrated;
    let task_data = if generic_task {
        Some(crate::tasks::save(
            &mut candidate,
            actor,
            &key,
            data,
            parent.as_deref(),
            position,
            same,
        )?)
    } else {
        None
    };
    // A change of only the test list is no change of the specification: the document gets a
    // revision, and the specification keeps its revision.
    let tests_only = w.test_rules_migrated
        && old.as_ref().is_some_and(|o| {
            let without = |d: &Value| {
                let mut d = d.clone();
                if let Some(map) = d.as_object_mut() {
                    map.remove("tests");
                }
                d
            };
            without(&o.current().data) == without(data) && o.current().parent == *parent
        });
    if (!unchanged && !tests_only || schema(w).types[kind].role == "requirement") && !generic_task {
        if let Some(cmd) = typed_command(w, kind, id, parent.as_deref(), data, reason)? {
            candidate.document_overrides.remove(&key);
            commands::apply_inner(&mut candidate, actor, &cmd, position, at)?;
            if let Command::Revise { req, .. } = &cmd
                && commands::rule_applies(&candidate, commands::KEEP_ASSESSMENTS, position)
            {
                keep_assessments(&mut candidate, req);
            }
            // A criterion save changes only that criterion: its siblings keep their revisions.
            let siblings = schema(w).types[kind].role != "ac"
                || !commands::rule_applies(&candidate, commands::SINGLE_CRITERION_SAVE, position);
            // The specification revision names the criterion save that made it.
            let spec_reason = (schema(w).types[kind].role == "ac"
                && commands::rule_applies(&candidate, commands::CRITERION_SAVE_REASON, position))
            .then(|| format!("Criterion {key} changed: {reason}"));
            sync_with(
                &mut candidate,
                actor,
                &cmd,
                position,
                at,
                siblings,
                spec_reason.as_deref(),
            );
        } else {
            candidate.document_overrides.insert(key.clone());
            let owner = string(data, "owner")?;
            commands::human(w, &owner)?;
            if let Some(o) = &old {
                ensure!(
                    o.current().data["owner"] == owner
                        || o.current().data["owner"] == actor
                        || actor == w.admin,
                    "only current owner can transfer responsibility"
                );
            }
            if ["project", "intake", "point"].contains(&kind.as_str()) && !w.schema_migrated {
                ensure!(
                    old.is_some(),
                    "create source data with save_object kind=note"
                );
                ensure!(
                    actor == w.admin
                        || old
                            .as_ref()
                            .is_some_and(|o| o.current().data["owner"] == actor),
                    "source owner required"
                );
                match kind.as_str() {
                    "project" => {
                        ensure!(
                            string(data, "title")? == *id,
                            "project title is its stable identifier"
                        );
                        candidate
                            .projects
                            .insert(id.clone(), string(data, "repository")?);
                    }
                    "intake" => {
                        let i = candidate.intake.get_mut(id).context("unknown intake")?;
                        ensure!(
                            data["author"] == i.author && data["title"] == *id,
                            "source author and id cannot change"
                        );
                        i.said = string(data, "body")?;
                        i.owner = Some(owner.clone());
                    }
                    "point" => {
                        let (source, point) = id.split_once('/').context("invalid source point")?;
                        let p = candidate
                            .intake
                            .get_mut(source)
                            .and_then(|i| i.points.get_mut(point))
                            .context("unknown source point")?;
                        ensure!(
                            old.as_ref().unwrap().current().data["owner"] == owner,
                            "point inherits its source owner"
                        );
                        for field in ["kind", "answers", "outcome", "reason", "requirement"] {
                            ensure!(
                                data[field] == old.as_ref().unwrap().current().data[field],
                                "use source process commands for {field}"
                            );
                        }
                        p.said = string(data, "title")?;
                    }
                    _ => unreachable!(),
                }
                sync(&mut candidate, actor, command, position, at);
            }
        }
    }
    let schema_revision = schema(w).revision;
    let projected = candidate
        .objects
        .get(&key)
        .map(|o| o.current().data.clone())
        .unwrap_or_else(|| data.clone());
    // A custom parent changes grouping. The legacy semantic ancestor remains explicit.
    let mut object = candidate
        .objects
        .get(&key)
        .cloned()
        .unwrap_or_else(|| Object {
            id: id.clone(),
            kind: kind.clone(),
            revisions: vec![],
            parent_override: false,
        });
    object.revisions = old
        .as_ref()
        .map(|o| o.revisions.clone())
        .unwrap_or_default();
    let mut saved_data = if let Some(data) = task_data {
        data
    } else if !candidate.document_overrides.contains(&key)
        && (w.documents_migrated
            && matches!(schema(w).types[kind].role.as_str(), "requirement" | "ac")
            || !w.documents_migrated && legacy_defaults().contains_key(kind) && kind != "note")
    {
        if w.schema_migrated {
            let mut normalized = projected;
            normalized.as_object_mut().unwrap().retain(|field, _| {
                data.get(field).is_some()
                    || document_adapter_fields(w, kind)
                        .is_some_and(|fields| fields.contains(&field.as_str()))
            });
            for (field, value) in data.as_object().unwrap() {
                normalized[field] = value.clone();
            }
            if !w.documents_migrated {
                for field in schema_migration_defaults()
                    .get(kind)
                    .into_iter()
                    .flat_map(|t| t.references.keys())
                {
                    if !schema(w).types[kind].references.contains_key(field)
                        && data.get(field).is_none()
                    {
                        normalized.as_object_mut().unwrap().remove(field);
                    }
                }
            }
            normalized
        } else {
            projected
        }
    } else {
        data.clone()
    };
    // A new signal is unprocessed until someone decides on it.
    if input_type(w, kind) && saved_data["status"].is_null() {
        saved_data["status"] = json!("unprocessed");
    }
    if input_type(w, kind)
        && let Some(old) = &old
    {
        let source = |value: &Value| {
            let mut value = value.clone();
            if let Some(fields) = value.as_object_mut() {
                fields.remove("status");
                fields.remove("owner");
                // A decision is a record about the signal, not a change of the signal.
                fields.remove("decision");
            }
            value
        };
        if saved_data["status"] == "processed" && source(&old.current().data) != source(&saved_data)
        {
            saved_data["status"] = json!("unprocessed");
        }
    }
    if same(&saved_data) {
        return Ok(None);
    }
    object.parent_override = document_rows(&candidate)
        .get(&key)
        .is_none_or(|(_, p, _)| p != parent);
    object.revisions.push(ObjectRevision {
        revision: revision + 1,
        schema_revision,
        parent: parent.clone(),
        data: saved_data,
        links: links.clone(),
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.clone(),
        present: true,
        contract: None,
    });
    candidate.objects.insert(key.clone(), object);
    validate_links(&candidate, &key, links, &kept)?;
    for (address, obj) in &w.objects {
        if !obj.current().present || !within(w, address, &key) {
            continue;
        }
        if let Some((req, _, _)) = criterion_context(w, address) {
            ensure!(
                criterion_context(&candidate, address).is_some_and(|(next, _, _)| next == req),
                "a parent change cannot move existing criterion evidence to another specification"
            );
        }
    }
    Ok(Some(candidate))
}

/// Reset the criteria below each object whose revision at `position` changed its title,
/// text, links or parent. Task and inbox revisions reset nothing. A blocking dependency
/// only orders work, so a change of those links alone resets nothing.
pub fn reset_changed(w: &mut Workspace, position: u64, at: &str) -> Result<()> {
    let types = schema(w).types;
    let migrated = w.test_rules_migrated;
    let content = |links: &[ObjectLink]| -> Vec<ObjectLink> {
        links
            .iter()
            .filter(|l| !(l.blocking && l.relation == "depends_on"))
            .cloned()
            .collect()
    };
    let changed: Vec<String> = w
        .objects
        .iter()
        .filter(|(_, o)| {
            o.current().position == position
                && types.get(&o.kind).is_some_and(|t| t.role != "task")
                && !input_type(w, &o.kind)
        })
        .filter(|(_, o)| {
            let current = o.current();
            o.revisions
                .iter()
                .rev()
                .find(|r| r.position < position)
                .is_some_and(|before| {
                    before.parent != current.parent
                        || content(&before.links) != content(&current.links)
                        || TEXT_FIELDS
                            .iter()
                            .any(|field| before.data.get(field) != current.data.get(field))
                        // A new test list resets the criterion. The migration moves a list
                        // into the document and changes no status.
                        || (migrated
                            && before.data.get("tests") != current.data.get("tests")
                            && current.reason != crate::test_rules::REASON)
                })
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in changed {
        reset_criteria(w, &key, at)?;
    }
    Ok(())
}

/// Reset each present criterion at and below `root` after a change recorded at `at`.
/// Reports received at or before `at` stop counting for it, and its readiness
/// assessment is cleared. A later boundary replaces an earlier one.
pub fn reset_criteria(w: &mut Workspace, root: &str, at: &str) -> Result<()> {
    let root = resolve_address(w, root).to_owned();
    let types = schema(w).types;
    let criteria: Vec<String> = w
        .objects
        .iter()
        .filter(|(key, o)| {
            o.current().present
                && types.get(&o.kind).is_some_and(|t| t.role == "ac")
                && within(w, key, &root)
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in criteria {
        if let Some((req, r, ac)) = criterion_context(w, &key) {
            let (req, number, id) = (req.to_owned(), r.number, ac.id.clone());
            let r = commands::revision(w, &req, number, true)?;
            let ac = r
                .acs
                .iter_mut()
                .find(|a| a.id == id)
                .context("unknown AC")?;
            if ac.assessment.take().is_some() {
                r.acceptance = None;
            }
        }
        // An earlier or equal time keeps the stored boundary.
        let later = w.criterion_boundaries.get(&key).is_some_and(|old| {
            use crate::ac_tests::Boundary;
            matches!(
                (Boundary::read(Some(old)), Boundary::read(Some(at))),
                (Boundary::After(old), Boundary::After(new)) if old >= new
            )
        });
        if !later {
            w.criterion_boundaries.insert(key, at.into());
        }
    }
    Ok(())
}

/// A Task pin stays current across process events, which keep its authored origin revision.
pub fn pin_current(w: &Workspace, target: &str, pin: Option<u32>) -> bool {
    let key = resolve_address(w, target);
    let (Some(pin), Some(o)) = (pin, w.objects.get(key)) else {
        return false;
    };
    // The last revisions that change only the test list keep a pin current.
    let (contract, current) = (contract_revision(o), o.current().revision);
    o.current().present
        && w.task_process
            .get(key)
            .map_or(contract <= pin && pin <= current, |t| {
                t.retired.is_none()
                    && t.origin_revision >= 1
                    && t.origin_revision <= pin
                    && pin <= current
            })
}
/// The revision of the contract of an object: the current revision before the last
/// revisions that change only the test list.
pub fn contract_revision(o: &Object) -> u32 {
    contract_of(o.current())
}
fn contract_of(r: &ObjectRevision) -> u32 {
    match r.contract {
        Some([revision, contract]) if revision == r.revision => contract,
        _ => r.revision,
    }
}
/// The revision `after` changes only the field tests of `before`.
pub fn tests_only_change(before: &ObjectRevision, after: &ObjectRevision) -> bool {
    let without = |d: &Value| {
        let mut d = d.clone();
        if let Some(map) = d.as_object_mut() {
            map.remove("tests");
        }
        d
    };
    before.data.get("tests") != after.data.get("tests")
        && before.parent == after.parent
        && before.links == after.links
        && before.present == after.present
        && without(&before.data) == without(&after.data)
}
/// A Task type: its dependencies pin the target contract.
pub fn task_kind(w: &Workspace, kind: &str) -> bool {
    schema(w).types.get(kind).is_some_and(|t| t.role == "task")
}
/// The Task dependencies whose pinned revision is no longer current. The dependencies of
/// another node order work and pin nothing, so a stored revision there needs no review.
pub fn review_links(w: &Workspace, key: &str) -> Vec<Value> {
    w.objects.get(resolve_address(w, key)).filter(|o| task_kind(w, &o.kind)).map(|o|o.current().links.iter().filter_map(|link| {
        let current=w.objects.get(resolve_address(w, &link.target)).filter(|o|o.current().present).map(|o|o.current().revision);
        (link.relation=="depends_on" && !pin_current(w, &link.target, link.revision)).then(||json!({"target":link.target,"revision":link.revision,"current_revision":current}))
    }).collect()).unwrap_or_default()
}
/// Task dependency pins of the object and its ancestors. The ordering links of other nodes
/// wait at read time and in live writes, where the test status of their targets is known.
/// A criterion with a saved test list: its test status from reports decides a link to it.
pub fn report_decided(w: &Workspace, key: &str) -> bool {
    let key = resolve_address(w, key);
    w.objects.get(key).is_some_and(|o| {
        schema(w).types.get(&o.kind).is_some_and(|t| {
            if w.test_rules_migrated {
                status_rule(t) == "tests"
            } else {
                t.role == "ac"
            }
        })
    }) && crate::ac_tests::listed(w, key).is_some_and(|r| !r.tests.is_empty())
}

pub fn work_ready(w: &Workspace, key: &str) -> bool {
    fn visit(
        w: &Workspace,
        key: &str,
        types: &BTreeMap<String, TreeType>,
        stack: &mut BTreeSet<String>,
    ) -> bool {
        let key = resolve_address(w, key);
        let Some(o) = w.objects.get(resolve_address(w, key)) else {
            return false;
        };
        let task = types.get(&o.kind).is_some_and(|t| t.role == "task");
        let mut links = o.current().links.iter().filter(|l| task && l.blocking);
        if !o.current().present
            || links
                .clone()
                .any(|l| !pin_current(w, &l.target, l.revision))
            || !stack.insert(key.into())
        {
            return false;
        }
        let links_ready = links.all(|l| {
            let Some(target) = w.objects.get(resolve_address(w, &l.target)) else {
                return false;
            };
            // A criterion with a test list is decided at read time by its test status from
            // reports (dependencies::Graph). Any other target keeps its recorded evidence.
            let target_key = address(&target.kind, &target.id);
            let evidence = report_decided(w, &target_key) || verified(w, &target_key);
            evidence && visit(w, &l.target, types, stack)
        });
        let parent_ready = o
            .current()
            .parent
            .as_deref()
            .is_none_or(|p| visit(w, p, types, stack));
        stack.remove(key);
        links_ready && parent_ready
    }
    visit(w, key, &schema(w).types, &mut BTreeSet::new())
}

/// The first direct blocking dependency that keeps the object out of work.
pub fn unready_dependency(w: &Workspace, key: &str) -> Option<String> {
    let o = w.objects.get(resolve_address(w, key))?;
    o.current()
        .links
        .iter()
        .filter(|l| l.blocking)
        .find(|l| {
            !pin_current(w, &l.target, l.revision)
                || w.objects
                    .get(resolve_address(w, &l.target))
                    .is_none_or(|t| !verified(w, &address(&t.kind, &t.id)))
                || !work_ready(w, &l.target)
        })
        .map(|l| resolve_address(w, &l.target).to_owned())
}

fn legacy_task_binding(w: &Workspace, obj: &Object) -> bool {
    obj.kind == "task"
        && obj.id.split_once('/').is_some_and(|(req, id)| {
            obj.current().parent.as_deref() == Some(&requirement_address(req))
                && w.requirements
                    .get(req)
                    .and_then(|q| q.revisions.last())
                    .and_then(|r| r.tasks.get(id))
                    .is_some_and(|t| {
                        t.acs.len() != 1 && obj.current().data["legacy_acs"] == json!(t.acs)
                    })
        })
}

/// New writes validate the active schema. Replay preserves earlier schema rules.
pub fn validate_tree(w: &Workspace) -> Result<()> {
    let types = schema(w).types;
    for (key, o) in w.objects.iter().filter(|(_, o)| o.current().present) {
        ensure!(
            types.contains_key(&o.kind),
            "object type is absent from the tree schema"
        );
        validate_required_fields(&types[&o.kind], &o.current().data)?;
        if !legacy_task_binding(w, o) {
            parent_valid(&types, w, &o.kind, o.current().parent.as_deref(), Some(key))?;
        }
        parent_cycle(w, key, o.current().parent.as_deref())?;
    }
    Ok(())
}

pub fn requirement_summary(w: &Workspace, req: &str, r: &Revision) -> Value {
    let mut summary = crate::checks::summary(r);
    let mut blocked = false;
    for ac in &r.acs {
        let key = criterion_address(&ac.id);
        if !w.objects.is_empty() && !work_ready(w, &key) {
            summary["acs"][&ac.id]["ready"] = json!(false);
            summary["acs"][&ac.id]["verification"] = json!("n/a");
            summary["acs"][&ac.id]["review_required"] = json!(review_links(w, &key));
            blocked = true;
        }
    }
    if blocked || !w.objects.is_empty() && !work_ready(w, &requirement_address(req)) {
        summary["ready_for_acceptance"] = json!(false);
        summary["accepted"] = json!(false);
    }
    summary
}

pub fn within(w: &Workspace, key: &str, target: &str) -> bool {
    let target = resolve_address(w, target);
    let mut current = Some(key);
    let mut seen = BTreeSet::new();
    while let Some(id) = current {
        let id = resolve_address(w, id);
        if id == target {
            return true;
        }
        if !seen.insert(id) {
            return false;
        }
        current = w
            .objects
            .get(resolve_address(w, id))
            .and_then(|o| o.current().parent.as_deref());
    }
    false
}

/// Default title length when a type does not set `title_preview_length`.
pub const TITLE_LENGTH_DEFAULT: u32 = 64;
/// A new title fits its row: it is at most the type's `title_preview_length`.
/// Only new writes are checked, so old journals with long titles still replay.
fn validate_title_length(w: &Workspace, cmd: &Command) -> Result<()> {
    let value = serde_json::to_value(cmd)?;
    let kind = match value["op"].as_str().unwrap_or_default() {
        "save_object" => value["kind"].as_str().unwrap_or_default(),
        "domain" | "edit_domain" => "domain",
        "capability" | "edit_capability" => "capability",
        "consumer" | "edit_consumer" => "consumer",
        "revise" => "requirement",
        _ => return Ok(()),
    };
    let types = schema(w).types;
    let limit = |kind: &str| {
        types
            .get(kind)
            .and_then(|t| t.title_preview_length)
            .unwrap_or(TITLE_LENGTH_DEFAULT) as usize
    };
    let mut titles = vec![(
        kind,
        value.get("title").or_else(|| value.pointer("/data/title")),
    )];
    if let Some(acs) = value.get("acs").and_then(Value::as_array) {
        titles.extend(acs.iter().map(|ac| ("ac", ac.get("text"))));
    }
    for (kind, title) in titles {
        if let Some(title) = title.and_then(Value::as_str) {
            let limit = limit(kind);
            let article = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            ensure!(
                title.chars().count() <= limit,
                "{article} {kind} title is at most {limit} characters; put the details in the text"
            );
        }
    }
    Ok(())
}
/// A live save may add a blocking Task link only to a Task under the same requirement.
/// Replay skips this check, so stored links keep loading.
fn task_links_same_requirement(w: &Workspace, cmd: &Command) -> Result<()> {
    let Command::SaveObject {
        id,
        kind,
        parent,
        links,
        ..
    } = cmd
    else {
        return Ok(());
    };
    if !task_kind(w, kind) {
        return Ok(());
    }
    // A stored blocking link stays allowed when the save only moves its pin.
    let stored: BTreeSet<&str> = w
        .objects
        .get(resolve_address(w, &address(kind, id)))
        .into_iter()
        .flat_map(|o| o.current().links.iter().filter(|l| l.blocking))
        .map(|l| resolve_address(w, &l.target))
        .collect();
    // The candidate state: the Task sits under its proposed parent.
    let req = parent
        .as_deref()
        .and_then(|parent| crate::tasks::requirement(w, parent));
    for link in links
        .iter()
        .filter(|l| l.blocking && !stored.contains(resolve_address(w, &l.target)))
    {
        let Some(target) = w.objects.get(resolve_address(w, &link.target)) else {
            continue;
        };
        if task_kind(w, &target.kind) {
            ensure!(
                crate::tasks::requirement(w, &link.target) == req,
                "a Task can depend only on a Task under the same requirement"
            );
        }
    }
    Ok(())
}
/// A live save refuses a new blocking link from S to T when T, or a node below T, waits on S,
/// on an ancestor of S or on a node below S. A node waits on the blocking link targets of
/// itself and of its ancestors, and a target needs itself and each live node below it.
/// Task links keep their revision pins and do not take part, as in `dependencies::Graph`.
/// Replay accepts the links that are already stored.
fn validate_wait_cycle(
    w: &Workspace,
    key: &str,
    parent: Option<&str>,
    links: &[ObjectLink],
) -> Result<()> {
    let task = |k: &str| {
        k.split_once(':')
            .is_some_and(|(kind, _)| task_kind(w, kind))
    };
    if task(key) {
        return Ok(());
    }
    let source = resolve_address(w, key).to_owned();
    let stored = w
        .objects
        .get(&source)
        .map(|o| o.current().links.as_slice())
        .unwrap_or_default();
    let added: Vec<String> = links
        .iter()
        .filter(|l| l.blocking && !stored.iter().any(|s| s.blocking && s.target == l.target))
        .map(|l| resolve_address(w, &l.target).to_owned())
        .collect();
    if added.is_empty() {
        return Ok(());
    }
    let live = |k: &str| w.objects.get(k).is_some_and(|o| o.current().present);
    let parent_of = |k: &str| -> Option<String> {
        if k == source {
            parent.map(|p| resolve_address(w, p).to_owned())
        } else {
            w.objects
                .get(k)
                .and_then(|o| o.current().parent.as_deref())
                .map(|p| resolve_address(w, p).to_owned())
        }
    };
    let mut children = BTreeMap::<String, Vec<String>>::new();
    for k in w.objects.keys().filter(|k| live(k)).chain([&source]) {
        if let Some(p) = parent_of(k) {
            children.entry(p).or_default().push(k.clone());
        }
    }
    let ancestors = |k: &str| {
        let mut found = vec![];
        let mut next = parent_of(k);
        while let Some(p) = next.filter(|p| *p != k && !found.contains(p)) {
            next = parent_of(&p);
            found.push(p);
        }
        found
    };
    let below = |k: &str| {
        let mut found = BTreeSet::new();
        let mut stack = vec![k.to_owned()];
        while let Some(n) = stack.pop() {
            for c in children.get(&n).into_iter().flatten() {
                if found.insert(c.clone()) {
                    stack.push(c.clone());
                }
            }
        }
        found
    };
    let targets = |k: &str| -> Vec<String> {
        let own = if task(k) {
            &[]
        } else if k == source {
            links
        } else {
            w.objects
                .get(k)
                .map(|o| o.current().links.as_slice())
                .unwrap_or_default()
        };
        own.iter()
            .filter(|l| l.blocking)
            .map(|l| resolve_address(w, &l.target).to_owned())
            .filter(|t| live(t))
            .collect()
    };
    let mut cycle: BTreeSet<String> = below(&source);
    cycle.extend(ancestors(&source));
    cycle.insert(source.clone());
    let mut visited = BTreeSet::new();
    let mut needed = added;
    while let Some(target) = needed.pop() {
        ensure!(!cycle.contains(&target), "dependency cycle");
        let mut nodes = below(&target);
        nodes.insert(target);
        for node in nodes {
            if visited.insert(node.clone()) {
                for waiting in std::iter::once(node.clone()).chain(ancestors(&node)) {
                    needed.extend(targets(&waiting));
                }
            }
        }
    }
    Ok(())
}
pub fn validate_current(w: &Workspace, cmd: &Command) -> Result<()> {
    validate_title_length(w, cmd)?;
    task_links_same_requirement(w, cmd)?;
    if let Command::SaveObject {
        kind,
        id,
        parent,
        links,
        ..
    } = cmd
    {
        validate_wait_cycle(w, &address(kind, id), parent.as_deref(), links)?;
    }
    match cmd {
        Command::MigrateNotes {}
        | Command::MigrateTasks {}
        | Command::MigrateSchema {}
        | Command::MigrateDocuments {}
        | Command::MigrateTaskClaims {}
        | Command::MigrateConsumers {}
        | Command::MigrateConsumerDrops {}
        | Command::MigrateTestRules {} => {
            bail!("internal migration operation")
        }
        Command::Intake { .. }
        | Command::Split { .. }
        | Command::Questions { .. }
        | Command::Sort { .. }
        | Command::Answer { .. }
        | Command::Route { .. } => {
            bail!("use save_object kind=note; incoming source types are removed")
        }
        Command::SaveObject {
            kind, id, revision, ..
        } if !w.documents_migrated => {
            ensure!(
                !["intake", "point"].contains(&kind.as_str()),
                "use kind=note"
            );
            ensure!(
                kind != "note" || *revision != 0 || !id.starts_with("source-"),
                "Note migration identifiers are reserved"
            );
        }
        Command::SetTreeSchema {
            types, put_types, ..
        } if !w.documents_migrated => ensure!(
            [types, put_types]
                .into_iter()
                .flatten()
                .all(|t| !t.contains_key("intake") && !t.contains_key("point")),
            "incoming source types are removed; use Note"
        ),
        Command::Task { acs, .. } => ensure!(acs.len() == 1, "Task must belong to exactly one AC"),
        Command::CompleteTask {
            req, id, commit, ..
        } => {
            ensure!(
                !w.test_rules_migrated || commit.is_some(),
                "missing field `commit`: complete_task needs the git sha where the Task is ready"
            );
            ensure!(
                work_ready(w, &crate::tasks::address(req, id)),
                "Task has an unmet dependency or a changed dependency revision"
            )
        }
        Command::ReviewTask {
            req,
            revision,
            id,
            ac,
            ..
        } => {
            if req.is_empty() {
                return Ok(());
            }
            let r = w
                .requirements
                .get(req)
                .and_then(|q| q.revisions.last())
                .context("unknown REQ")?;
            ensure!(r.number == *revision, "stale REQ revision");
            let task = r.tasks.get(id).context("unknown Task")?;
            ensure!(
                task.done.is_none() || task.acs == vec![ac.clone()],
                "completed Task cannot move to another AC"
            );
            ensure!(task.retired.is_none(), "retired Task cannot be reviewed");
        }
        Command::Accept { req, .. } => {
            let r = w
                .requirements
                .get(req)
                .and_then(|q| q.revisions.last())
                .context("unknown REQ")?;
            ensure!(
                requirement_summary(w, req, r)["ready_for_acceptance"] == true,
                "REQ has unverified ACs or an unmet dependency"
            );
        }
        Command::EditDomain { id, .. } | Command::SetDomainArchived { id, .. } => ensure!(
            !w.archived_objects.contains(&address("domain", id)),
            "restore the archived object before editing it"
        ),
        Command::EditCapability { id, .. } => ensure!(
            !w.archived_objects.contains(&address("capability", id)),
            "restore the archived object before editing it"
        ),
        _ => {}
    }
    Ok(())
}
fn criterion_record_context<'a>(
    w: &'a Workspace,
    key: &str,
) -> Option<(&'a str, &'a Revision, &'a Ac)> {
    let key = resolve_address(w, key);
    if let Some(context) = criterion_context(w, key) {
        return Some(context);
    }
    let obj = w.objects.get(key)?;
    if obj.current().present {
        return None;
    }
    for (req, q) in &w.requirements {
        if let Some(ac) = q
            .revisions
            .iter()
            .rev()
            .flat_map(|r| r.acs.iter())
            .find(|ac| criterion_address(&ac.id) == key)
        {
            return Some((req, q.revisions.last()?, ac));
        }
    }
    None
}

pub fn annotation_address(w: &Workspace, key: &str) -> String {
    let key = resolve_address(w, key);
    if let Some((req, _, ac)) = criterion_record_context(w, key)
        && !ac.id.contains(':')
        && !req.contains(':')
    {
        return format!("ac:{req}/{}", ac.id);
    }
    key.into()
}

/// Resolve the configured verification role against the journal evidence adapter.
pub fn verification_context<'a>(w: &'a Workspace, key: &str) -> Option<(&'a str, &'a Revision)> {
    let mut current = Some(resolve_address(w, key));
    let mut seen = BTreeSet::new();
    while let Some(key) = current {
        if !seen.insert(key) {
            return None;
        }
        let o = w.objects.get(key)?;
        let role = schema(w).types.get(&o.kind).map(|t| t.role.clone());
        if role.as_deref() == Some("requirement")
            && !w.document_overrides.contains(key)
            && let Some(req) = requirement_key(w, key)
            && let Some(r) = w.requirements.get(req).and_then(|q| q.revisions.last())
        {
            return Some((req, r));
        }
        current = o.current().parent.as_deref();
    }
    None
}
pub fn criterion_context<'a>(
    w: &'a Workspace,
    key: &str,
) -> Option<(&'a str, &'a Revision, &'a Ac)> {
    let key = resolve_address(w, key);
    let o = w.objects.get(key)?;
    if w.document_overrides.contains(key) || schema(w).types.get(&o.kind)?.role != "ac" {
        return None;
    }
    let (req, revision) = verification_context(w, o.current().parent.as_deref()?)?;
    let ac = revision
        .acs
        .iter()
        .find(|ac| criterion_address(&ac.id) == key)?;
    Some((req, revision, ac))
}

/// Read the assessment from the current configured verification context.
pub fn assessment(w: &Workspace, key: &str) -> Option<Value> {
    let (req, r, ac) = criterion_context(w, key)?;
    let state = match ac.assessment.as_ref() {
        None => "unassessed",
        Some(a) if a.ready => "ready",
        Some(_) => "waiting",
    };
    let mut out = json!({"state":state,"req":req,"revision":r.number,"criterion":ac.id,
        "reason":ac.assessment.as_ref().map(|a| &a.reason),
        "recipient":ac.assessment.as_ref().and_then(|a| a.owner.as_ref()),
        "decision":ac.assessment.as_ref().and_then(|a| a.decision.as_ref())});
    if state == "unassessed" {
        // The next step: the command that sets readiness, without the assessment itself.
        out["next"] = json!({"op":"assess","req":req,"revision":r.number,"ac":ac.id});
    }
    Some(out)
}

/// Return the first assessment that prevents readiness, with a cycle guard.
pub fn readiness_blocker(w: &Workspace, key: &str, include_self: bool) -> Option<Value> {
    fn visit<'a>(r: &'a Revision, ac: &'a Ac, seen: &mut BTreeSet<&'a str>) -> Option<&'a Ac> {
        if !seen.insert(&ac.id) {
            return None;
        }
        if !ac.assessment.as_ref().is_some_and(|a| a.ready) {
            return Some(ac);
        }
        ac.depends_on.iter().find_map(|id| {
            r.acs
                .iter()
                .find(|a| &a.id == id)
                .and_then(|a| visit(r, a, seen))
        })
    }
    let (_, r, ac) = criterion_context(w, key)?;
    let mut seen = BTreeSet::new();
    let blocker = if include_self {
        visit(r, ac, &mut seen)
    } else {
        ac.depends_on.iter().find_map(|id| {
            r.acs
                .iter()
                .find(|a| &a.id == id)
                .and_then(|a| visit(r, a, &mut seen))
        })
    }?;
    assessment(w, &criterion_address(&blocker.id))
}

pub fn assessment_reason(a: &Value) -> String {
    if a["state"] == "unassessed" {
        format!(
            "Criterion {} has no readiness assessment",
            a["criterion"].as_str().unwrap_or("")
        )
    } else {
        format!(
            "{} · Recipient: {}",
            a["reason"]
                .as_str()
                .unwrap_or("Readiness decision required"),
            a["recipient"].as_str().unwrap_or("Unassigned")
        )
    }
}
fn verified(w: &Workspace, key: &str) -> bool {
    if let Some(t) = crate::tasks::task(w, key) {
        return t.done.is_some() && t.retired.is_none();
    }
    if let Some((_, r, ac)) = criterion_context(w, key) {
        return crate::checks::latest(r)
            .is_some_and(|(_, run)| crate::checks::verdict(r, run, &ac.id) == "pass");
    }
    let Some(o) = w.objects.get(resolve_address(w, key)) else {
        return false;
    };
    !w.document_overrides.contains(resolve_address(w, key))
        && schema(w)
            .types
            .get(&o.kind)
            .is_some_and(|t| t.role == "requirement")
        && verification_context(w, key)
            .map(|(_, r)| r)
            .is_some_and(|r| crate::checks::summary(r)["ready_for_acceptance"] == true)
}

pub fn launch_objects(w: &Workspace) -> Vec<&str> {
    let configured = schema(w);
    w.objects
        .iter()
        .filter(|(_, o)| {
            o.current().present
                && configured.types.get(&o.kind).is_some_and(|t| {
                    t.launch_blocker
                        .unwrap_or(!w.documents_migrated && o.kind == "ac")
                })
                && o.current().data["before_launch"] == true
        })
        .map(|(key, _)| key.as_str())
        .collect()
}

pub fn launch_summary(w: &Workspace) -> Value {
    let selected = launch_objects(w);
    let blockers: Vec<Value> = selected
        .iter()
        .filter_map(|key| {
            let context = verification_context(w, key);
            if context.is_some_and(|(req, r)| requirement_summary(w, req, r)["accepted"] == true) {
                return None;
            }
            let mut item = json!({"address":key});
            if let Some((req, r)) = context {
                item["req"] = json!(req);
                item["revision"] = json!(r.number);
                if let Some((_, _, ac)) = criterion_context(w, key) {
                    item["ac"] = json!(ac.id);
                }
            }
            Some(item)
        })
        .collect();
    json!({"configured":!selected.is_empty(),"ready":!selected.is_empty()&&blockers.is_empty(),"blocker_count":blockers.len(),"blockers":blockers,"meaning":"Configured launch conditions require owner acceptance of current check evidence."})
}

pub fn input_type(w: &Workspace, kind: &str) -> bool {
    schema(w)
        .types
        .get(kind)
        .is_some_and(|t| t.inbox.unwrap_or(!w.documents_migrated && kind == "note"))
}

pub fn verification(w: &Workspace, key: &str) -> &'static str {
    let key = resolve_address(w, key);
    if !work_ready(w, key) {
        return "unverified";
    }
    if crate::tasks::task(w, key).is_some() {
        return crate::checks::task_verdict(w, key);
    }
    if let Some((req, r, ac)) = criterion_context(w, key) {
        let summary = requirement_summary(w, req, r);
        return match summary["acs"][&ac.id]["verification"].as_str() {
            Some("pass") => "pass",
            Some("fail") => "fail",
            _ => "unverified",
        };
    }
    if let Some(o) = w.objects.get(key)
        && schema(w)
            .types
            .get(&o.kind)
            .is_some_and(|t| t.role == "requirement")
        && let Some((req, r)) = verification_context(w, key)
    {
        let summary = requirement_summary(w, req, r);
        if summary["ready_for_acceptance"] == true {
            return "pass";
        }
        if summary["acs"]
            .as_object()
            .is_some_and(|acs| acs.values().any(|a| a["verification"] == "fail"))
        {
            return "fail";
        }
    }
    "unverified"
}

pub fn process(w: &Workspace, key: &str) -> Value {
    let key = resolve_address(w, key);
    if let Some(mut p) = crate::tasks::process(w, key) {
        // After migrate_test_rules a Task shows only its own state and its commit.
        if !w.test_rules_migrated {
            p["verification"] = json!(verification(w, key));
        }
        return p;
    }
    let Some(o) = w.objects.get(key) else {
        return json!({});
    };
    if input_type(w, &o.kind) {
        let mut p = json!({"status":o.current().data["status"].as_str().unwrap_or("unprocessed"),"verification":"unverified"});
        if let Some(a) = w.signal_assessments.get(key) {
            p["assessment"] = json!({"state":if a.ready {"ready"} else {"not_ready"},"reason":a.reason,"actor":a.actor,"criterion":key});
        }
        return p;
    }
    let role = schema(w).types.get(&o.kind).map(|t| t.role.clone());
    let mut result=match role.as_deref() {
        Some("requirement") if !w.document_overrides.contains(key) => verification_context(w,key).map(|(req,r)|json!({"req":req,"revision":r.number,"confirmed":r.confirmed,"summary":requirement_summary(w,req,r)})).unwrap_or_else(||json!({})),
        Some("ac") => criterion_record_context(w, key).map(|(req,r,ac)|json!({"req":req,"revision":r.number,"confirmed":r.confirmed,"pending_run":requirement_summary(w,req,r)["pending_run"],"criterion":ac.id,"status":requirement_summary(w,req,r)["acs"][&ac.id]})).unwrap_or_else(||json!({})),
        Some("task") => o.id.split_once('/').and_then(|(req,id)|w.requirements.get(req).and_then(|q|q.revisions.last()).and_then(|r|r.tasks.get(id).map(|t|(req,r,t)))).map(|(req,r,t)|json!({"req":req,"revision":r.number,"assignee":t.assignee,"done":t.done,"retired":t.retired,"review_required":t.review_required,"status":if t.retired.is_some(){"retired"}else if t.done.is_some(){"done"}else if t.assignee.is_some(){"in_progress"}else{"open"}})).unwrap_or_else(||json!({})),
        _ => json!({}),
    };
    result["verification"] = json!(verification(w, key));
    if role.as_deref() == Some("requirement") {
        result["acceptance"] = crate::acceptance::current(w, resolve_address(w, key));
    }
    // Only the tree schema adds the confirmation and the acceptance fields.
    let fields = process_fields(w, &o.kind);
    if let Some(map) = result.as_object_mut() {
        for field in ["confirmed", "acceptance"] {
            if !fields.iter().any(|f| f == field) {
                map.remove(field);
            }
        }
    }
    result["annotation_address"] = json!(annotation_address(w, key));
    if role.as_deref() == Some("ac") {
        result["planning_claim"] = json!(w.planning_claims.get(key));
        result["assessment"] = json!(assessment(w, key));
        result["readiness_blocker"] = json!(readiness_blocker(w, key, true));
    }
    result
}

pub fn signal(w: &Workspace, key: &str) -> Value {
    let Some(o) = w.objects.get(resolve_address(w, key)) else {
        return json!({});
    };
    let p = process(w, key);
    let (tone, label) = if p["status"] == "retired" {
        ("grey", "Retired")
    } else if !o.current().present {
        (
            "grey",
            "Object is absent from the current verification revision",
        )
    } else if p["review_required"] == true {
        ("purple", "Task contract needs owner review")
    } else if !review_links(w, key).is_empty() {
        ("purple", "Dependency revision changed")
    } else if !work_ready(w, key) {
        ("purple", "Dependency is not ready")
    } else if input_type(w, &o.kind) {
        if p["status"] == "processed" {
            ("grey", "Input processed")
        } else {
            ("yellow", "Input awaits processing")
        }
    } else if schema(w).types.get(&o.kind).is_some_and(|t| t.role == "ac")
        && p["pending_run"].is_string()
    {
        ("yellow", "New checks are running")
    } else if schema(w).types.get(&o.kind).is_some_and(|t| t.role == "ac") {
        match (
            p["status"]["ready"].as_bool(),
            p["status"]["verification"].as_str(),
        ) {
            (_, Some("fail")) => ("red", "Checks failed"),
            (Some(true), Some("pass")) => ("green", "Checks passed"),
            (Some(false), _) if p["readiness_blocker"]["state"] == "unassessed" => {
                ("yellow", "Readiness not assessed: send assess")
            }
            (Some(false), _) => ("yellow", "Readiness decision required"),
            (Some(true), _) if p["readiness_blocker"].is_null() => ("grey", "Ready for work"),
            _ => ("grey", "AC awaits checks"),
        }
    } else if schema(w)
        .types
        .get(&o.kind)
        .is_some_and(|t| t.role == "requirement")
    {
        if p["summary"]["acs"]
            .as_object()
            .is_some_and(|acs| acs.values().any(|a| a["verification"] == "fail"))
        {
            ("red", "AC checks failed")
        } else if p["summary"]["pending_run"].is_string() {
            ("yellow", "New checks are running")
        } else if p["summary"]["accepted"] == true {
            ("green", "REQ accepted")
        } else if p["summary"]["ready_for_acceptance"] == true {
            ("green", "AC checks passed")
        } else if p["confirmed"] == false {
            ("yellow", "REQ awaits confirmation")
        } else {
            ("grey", "REQ awaits checks")
        }
    } else if schema(w)
        .types
        .get(&o.kind)
        .is_some_and(|t| t.role == "task")
    {
        task_signal(&p)
    } else {
        ("grey", "Current document")
    };
    let mut signal = json!({"tone":tone,"label":label,"at":o.current().at});
    for field in ["assessment", "readiness_blocker"] {
        if p[field].is_object() {
            signal[field] = p[field].clone();
        }
    }
    signal
}

/// Tone and label of a Task whose signal is not decided by an earlier rule.
pub fn task_signal(p: &Value) -> (&'static str, &'static str) {
    if p.get("verification").is_none() {
        return match p["status"].as_str() {
            Some("done") => ("green", "Work complete"),
            Some("in_progress") => ("yellow", "Task in progress"),
            _ if p["readiness_blocker"].is_object() => {
                ("yellow", "Task waits for parent readiness")
            }
            _ => ("grey", "Task open"),
        };
    }
    match p["status"].as_str() {
        Some("done") if p["verification"] == "pass" => ("green", "Work complete · Checks passed"),
        Some("done") if p["verification"] == "fail" => ("red", "Work complete · Checks failed"),
        Some("done") => ("grey", "Work complete · Unverified"),
        Some("in_progress") => ("yellow", "Task in progress"),
        _ if p["readiness_blocker"].is_object() => ("yellow", "Task waits for parent readiness"),
        _ => ("grey", "Task open"),
    }
}

pub fn archive(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<()> {
    let Command::ArchiveObject {
        target,
        revision,
        reason,
    } = command
    else {
        bail!("archive_object required")
    };
    let target = resolve_address(w, target).to_owned();
    ensure!(may_edit_tree(w, actor), "workspace member required");
    commands::nonempty(reason)?;
    let old = w.objects.get(&target).context("unknown object")?;
    ensure!(
        old.current().revision == *revision,
        "stale object revision; read the object again"
    );
    ensure!(
        old.current().present,
        "object is absent from the current tree"
    );
    let kind = &old.kind;
    ensure!(
        schema(w)
            .types
            .get(kind)
            .is_some_and(|t| ["group", "document", "requirement", "ac"].contains(&t.role.as_str())),
        "archive_object requires a group, document, requirement or criterion"
    );

    ensure!(
        !w.planning_claims.keys().any(|ac| within(w, ac, &target)),
        "cannot archive an object with an active planning claim"
    );

    let has_live_children = w.objects.values().any(|o| {
        o.current().parent.as_ref() == Some(&target)
            && o.current().present
            && crate::tasks::task(w, &address(&o.kind, &o.id))
                .is_none_or(|task| task.retired.is_none() && task.done.is_none())
    });
    ensure!(
        !has_live_children,
        "cannot archive an object with active children"
    );

    let mut next = old.current().clone();
    next.present = false;
    next.schema_revision = schema(w).revision;
    next.revision += 1;
    next.position = position;
    next.actor = actor.to_owned();
    next.reason = reason.clone();
    next.at = at.to_owned();

    let parent = next.parent.clone();
    let inbox = input_type(w, kind);
    w.objects.get_mut(&target).unwrap().revisions.push(next);
    w.archived_objects.insert(target.clone());
    if !commands::rule_applies(w, commands::LIST_SAVE_RESET, position) {
        return Ok(());
    }
    // An archived spec object resets the criteria at and below its parent.
    match parent {
        Some(parent) if !inbox => reset_criteria(w, &parent, at),
        _ => Ok(()),
    }
}

pub fn restore(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<()> {
    let Command::RestoreObject {
        target,
        revision,
        reason,
    } = command
    else {
        bail!("restore_object required")
    };
    ensure!(may_edit_tree(w, actor), "workspace member required");
    commands::nonempty(reason)?;
    let target = resolve_address(w, target).to_owned();
    let old = w.objects.get(&target).context("unknown object")?;
    ensure!(
        old.current().revision == *revision,
        "stale object revision; read the object again"
    );
    ensure!(
        !old.current().present,
        "object is already present in the current tree"
    );
    ensure!(
        w.archived_objects.contains(&target),
        "object was not archived with archive_object"
    );

    let kind = &old.kind;
    ensure!(
        schema(w)
            .types
            .get(kind)
            .is_some_and(|t| ["group", "document"].contains(&t.role.as_str())),
        "restore_object requires a group or document"
    );

    parent_valid(
        &schema(w).types,
        w,
        kind,
        old.current().parent.as_deref(),
        Some(&target),
    )?;
    parent_cycle(w, &target, old.current().parent.as_deref())?;

    let mut next = old.current().clone();
    next.present = true;
    next.schema_revision = schema(w).revision;
    next.revision += 1;
    next.position = position;
    next.actor = actor.to_owned();
    next.reason = reason.clone();
    next.at = at.to_owned();

    w.objects.get_mut(&target).unwrap().revisions.push(next);
    w.archived_objects.remove(&target);
    Ok(())
}
