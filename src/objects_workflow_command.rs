//! Part of `objects`: `workflow_command` and the items after it, moved from objects.rs unchanged.
use super::*;

/// The typed command of one workflow for a saved document, or None for a type without one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn workflow_command(
    w: &Workspace,
    workflow: &str,
    kind: &str,
    id: &str,
    parent: Option<&str>,
    data: &Value,
    reason: &str,
    exists: bool,
) -> Result<Option<Command>> {
    // After migrate_body the typed description and contract come from data.body.
    let (text, contract) = (
        crate::body_migration::text_field(w, "description"),
        crate::body_migration::text_field(w, "contract"),
    );
    let description = || Ok::<_, anyhow::Error>(optional_string(data, text)?.unwrap_or_default());
    let cmd = match workflow {
        "domain" => {
            ensure!(
                data.get("archived").is_none_or(|v| Some(v)
                    == w.objects
                        .get(&address(kind, id))
                        .map(|o| &o.current().data["archived"])
                    || !exists && v == false),
                "use archive_object or restore_object to change archive state"
            );
            if exists {
                Command::EditDomain(crate::commands::EditDomainOp {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                })
            } else {
                Command::Domain(crate::commands::DomainOp {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                })
            }
        }
        "capability" => {
            if exists {
                Command::EditCapability(crate::commands::EditCapabilityOp {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: optional_string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                })
            } else {
                Command::Capability(crate::commands::CapabilityOp {
                    id: id.into(),
                    title: string(data, "title")?,
                    domain: ancestor(w, parent, "domain").ok(),
                    owner: optional_string(data, "owner")?,
                    description: description()?,
                })
            }
        }
        "consumer" => {
            if exists {
                Command::EditConsumer(crate::commands::EditConsumerOp {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                })
            } else {
                Command::Consumer(crate::commands::ConsumerOp {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                })
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
                revise(&req_key, r, capability, data, reason, fresh_acs(r))?
            } else {
                Command::Revise(crate::commands::ReviseOp {
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
                })
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
            // A criterion may have its own owner without a decision of that owner.
            let owner = (decision || assigned != r.owner).then_some(assigned);
            let ac_key = process_key(w, kind, id, "ac");
            let ac = Ac {
                id: ac_key.clone(),
                text: string(data, "title")?,
                description: description()?,
                decision_required: (owner.is_some() != decision).then_some(decision),
                owner,
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
                Command::AmendTask(crate::commands::AmendTaskOp {
                    req,
                    revision: r.number,
                    id: tid.into(),
                    title: string(data, "title")?,
                    contract: string(data, contract)?,
                    projects: list(data, "projects")?,
                    reason: reason.into(),
                })
            } else {
                Command::Task(crate::commands::TaskOp {
                    req,
                    revision: r.number,
                    id: tid.into(),
                    title: string(data, "title")?,
                    contract: string(data, contract)?,
                    projects: list(data, "projects")?,
                    purpose: string(data, "purpose")?,
                    acs: vec![ac_id],
                    assignee: None,
                })
            }
        }
        "project" | "intake" | "point" => return Ok(None),
        _ => return Ok(None),
    };
    Ok(Some(cmd))
}
