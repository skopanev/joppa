//! Part of `objects`: `schema_migration_defaults` and the items after it, moved from objects.rs unchanged.
use super::*;

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
                field_max_length: None,
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
                field_max_length: None,
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
