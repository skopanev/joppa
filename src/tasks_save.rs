//! Part of `tasks`: `save` and the items after it, moved from tasks.rs unchanged.
use super::*;

pub fn save(
    w: &mut Workspace,
    actor: &str,
    key: &str,
    data: &Value,
    parent: Option<&str>,
    position: u64,
) -> Result<Value> {
    let mut data = data.clone();
    let old = w.objects.get(key);
    let owner = data
        .get("owner")
        .and_then(Value::as_str)
        .or_else(|| old.and_then(|o| o.current().data["owner"].as_str()))
        .or_else(|| {
            parent
                .and_then(|p| w.objects.get(p))
                .and_then(|o| o.current().data["owner"].as_str())
        })
        .context("Task needs an owner")?
        .to_owned();
    commands::human(w, &owner)?;
    let string = |field: &str| {
        data[field]
            .as_str()
            .with_context(|| format!("missing {field}"))
    };
    // After migrate_body the Task contract is data.body.
    let text = crate::body_migration::text_field(w, "contract");
    commands::nonempty(string("title")?)?;
    commands::nonempty(string(text)?)?;
    let kind = key.split_once(':').context("Task address needs a type")?.0;
    let rules = objects::schema(w)
        .types
        .get(kind)
        .cloned()
        .context("unknown Task type")?;
    let mut projects: Vec<String> = if w.schema_migrated {
        objects::reference_targets(w, kind, &data)?
            .into_iter()
            .filter(|(_, _, subject)| *subject)
            .map(|(_, target, _)| target.split_once(':').unwrap().1.to_owned())
            .collect()
    } else {
        let projects: Vec<String> =
            serde_json::from_value(data.get("projects").cloned().unwrap_or_else(|| json!([])))?;
        ensure!(
            projects.iter().all(|p| w.projects.contains_key(p)),
            "unknown Task project"
        );
        projects
    };
    if w.schema_migrated
        && let Some(parent) = parent
    {
        projects.extend(subjects(w, parent)?);
        projects.sort();
        projects.dedup();
    }
    check_projects(w, parent, &projects)?;
    for field in data.as_object().context("data must be an object")?.keys() {
        ensure!(
            ["title", "owner", text].contains(&field.as_str())
                || field == "purpose" && !w.purpose_migrated
                || w.schema_migrated
                    && !objects::process_field(field)
                    && (w.documents_migrated || field != "projects")
                || w.schema_migrated
                    && rules.references.contains_key(field)
                    && !objects::process_field(field)
                || field == "projects"
                    && (!w.schema_migrated || data[field].as_array().is_some_and(Vec::is_empty)),
            "unknown Task document field {field}; use process commands for evidence"
        );
    }
    let title = string("title")?.to_owned();
    let contract = string(text)?.to_owned();
    data["owner"] = json!(owner);
    if !w.schema_migrated {
        data["projects"] = json!(projects);
    }
    let mut t = task(w, key).cloned().unwrap_or_else(|| Task {
        claim: None,
        title: String::new(),
        acs: vec![],
        projects: vec![],
        purpose: String::new(),
        assignee: None,
        done: None,
        commit: None,
        contract: String::new(),
        created_at: position,
        created_by: actor.into(),
        review_required: false,
        origin_revision: 1,
        retired: None,
        blocked: None,
    });
    t.title = title;
    t.contract = contract;
    t.projects = projects;
    t.acs.clear();
    t.origin_revision = old.map_or(1, |o| o.current().revision + 1);
    if old.is_none() {
        let pin = parent.and_then(|p| {
            w.objects.get(p).map(|o| {
                (
                    p.to_owned(),
                    w.task_process
                        .get(p)
                        .map_or(objects::contract_revision(o), |t| t.origin_revision),
                )
            })
        });
        w.task_context.insert(key.into(), pin);
    }
    w.task_process.insert(key.into(), t);
    Ok(data)
}
