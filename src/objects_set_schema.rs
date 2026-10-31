//! Part of `objects`: `set_schema` and the items after it, moved from objects.rs unchanged.
use super::*;

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
    crate::note_parents::check_schema(w, types)?;
    crate::body_migration::check_schema(w, types)?;
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
            "type title_max_length must be an integer from 1 to 256 characters"
        );
        ensure!(
            t.field_max_length.is_none_or(|length| {
                (1..=crate::field_limit::FIELD_CEILING as u32).contains(&length)
            }),
            "type field_max_length must be an integer from 1 to 262144 characters"
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
