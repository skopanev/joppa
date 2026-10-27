//! The event migrate_test_rules and the test rules that a workspace uses.
//!
//! The event runs once in each workspace. It gives each type of the tree schema its status
//! rule and its process fields by name, it moves each test list into the field tests of
//! its document, and it writes the full block test_results with the rules that the
//! workspace used before. After the event, the live code reads the rules only from the
//! schema. LEGACY_TEST_RULES holds the rules of the journals before the event: a replay of
//! an older event checks it against them, and the event writes them into the schema.
use crate::model::Workspace;
use crate::objects::{role_status_rule, schema};
use crate::result_shape::{NameRule, ResultField, ResultShape};
use anyhow::{Context, Result, ensure};

pub const REASON: &str = "Write the test rules and the status rule of each type into the schema";

/// The rules of every workspace before migrate_test_rules.
pub fn legacy() -> ResultShape {
    let outcomes = ["pass", "fail", "skip", "todo"];
    ResultShape {
        statuses: outcomes.map(|o| (o.to_owned(), o.to_owned())).into(),
        fields: vec![
            ResultField {
                name: "message".into(),
                title: Some("Failure message".into()),
                kind: "text".into(),
                required: false,
                values: vec![],
                max_length: Some(8 * 1024),
                board: false,
                target: None,
                applies: None,
            },
            ResultField {
                name: "duration_ms".into(),
                title: Some("Duration in milliseconds".into()),
                kind: "number".into(),
                required: false,
                values: vec![],
                max_length: None,
                board: false,
                target: None,
                applies: None,
            },
        ],
        name: NameRule {
            separator: " > ".into(),
            min_parts: 2,
            file_extension: true,
            max_length: 400,
        },
        max_tests: 200,
        max_results: 1000,
        same_commit: "fail_wins".into(),
        reset_on_change: true,
        green_if_all: vec!["pass".into()],
        red_if_any: vec!["fail".into()],
        failure_field: Some("message".into()),
    }
}

/// The fields whose change made an older test list a draft.
pub fn legacy_draft_fields() -> [&'static str; 2] {
    ["title", "description"]
}

/// The test rules of the workspace: the schema block, or the older rules before the event.
pub fn rules(w: &Workspace) -> Result<ResultShape> {
    if !w.test_rules_migrated {
        return Ok(legacy());
    }
    schema(w)
        .test_results
        .context("the tree schema has no test_results block")
}

/// Apply migrate_test_rules once. The workspace owner is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.test_rules_migrated {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    let mut next = schema(w);
    // Each type gets its status rule and its process fields by name.
    for (kind, t) in next.types.iter_mut() {
        t.status = Some(role_status_rule(t).to_owned());
        t.process_fields = Some(crate::objects::process_fields(w, kind));
    }
    // A block that an older journal saved keeps its status names and its own fields.
    let mut block = legacy();
    if let Some(old) = next.test_results.take() {
        if !old.statuses.is_empty() {
            block.statuses = old.statuses;
        }
        for mut field in old.fields {
            if field.kind == "text" && field.max_length.is_none() {
                field.max_length = Some(1024);
            }
            block.fields.push(field);
        }
    }
    next.test_results = Some(block);
    next.status_required = Some(true);
    next.revision += 1;
    next.actor = actor.into();
    next.at = at.into();
    next.position = position;
    next.reason = REASON.into();
    let schema_revision = next.revision;
    w.tree_schema.push(next);
    // Each current test list moves into the field tests of its document. The new revision
    // keeps the status: the boundary of the criterion does not change.
    let lists: Vec<(String, Vec<String>)> = w
        .ac_tests
        .iter()
        .filter_map(|(key, list)| {
            let tests = list.current()?.tests.clone();
            (!tests.is_empty() && w.objects.get(key)?.current().present)
                .then(|| (key.clone(), tests))
        })
        .collect();
    for (key, tests) in lists {
        let o = w.objects.get_mut(&key).expect("listed object");
        let mut revision = o.current().clone();
        let contract = crate::objects::contract_revision(o);
        revision.data["tests"] = serde_json::json!(tests);
        revision.revision += 1;
        revision.contract = Some([revision.revision, contract]);
        revision.schema_revision = schema_revision;
        revision.position = position;
        revision.actor = actor.into();
        revision.reason = REASON.into();
        revision.at = at.into();
        o.revisions.push(revision);
    }
    w.test_rules_migrated = true;
    Ok(())
}

/// Read the block of a set_tree_schema command. After migrate_test_rules, the block must
/// have every rule. A replay of an older command keeps the block that it saved.
pub fn parse_block(w: &Workspace, raw: &serde_json::Value) -> Result<ResultShape> {
    let shape: ResultShape = serde_json::from_value(raw.clone())
        .map_err(|e| anyhow::anyhow!("invalid test_results: {e}"))?;
    if w.test_rules_migrated {
        let missing: Vec<&str> = crate::result_shape::KEYS
            .into_iter()
            .filter(|key| raw.get(key).is_none())
            .collect();
        ensure!(
            missing.is_empty(),
            "test_results needs every rule; missing: {}",
            missing.join(", ")
        );
        crate::result_shape::validate(&shape)?;
    }
    Ok(shape)
}

/// Refuse a test name that does not have the form of the name rule.
pub fn validate_address(rules: &ResultShape, address: &str) -> Result<()> {
    let rule = &rules.name;
    let parts: Vec<&str> = address.split(rule.separator.as_str()).collect();
    let file = parts[0].rsplit('/').next().unwrap_or_default();
    let ok = parts.len() >= rule.min_parts
        && parts.iter().all(|part| !part.trim().is_empty())
        && (!rule.file_extension || (file.contains('.') && !file.starts_with('.')))
        && address.chars().count() <= rule.max_length
        && !address.chars().any(char::is_control);
    if rule.file_extension && rule.min_parts == 2 {
        ensure!(
            ok,
            "a test address needs file{}name at least",
            rule.separator
        );
    }
    ensure!(
        ok,
        "a test address needs {} parts joined by \"{}\" and at most {} characters",
        rule.min_parts,
        rule.separator,
        rule.max_length
    );
    Ok(())
}

/// The outcome name of a stored test status: skipped is the outcome skip.
pub fn outcome(status: &str) -> &str {
    match status {
        "skipped" => "skip",
        other => other,
    }
}
