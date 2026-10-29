//! Part of `objects`: `readiness_blocker` and the items after it, moved from objects.rs unchanged.
use super::*;

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

pub(crate) fn verified(w: &Workspace, key: &str) -> bool {
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
            // A legacy check-run acceptance or an acceptance on a commit settles the condition.
            if context.is_some_and(|(req, r)| requirement_summary(w, req, r)["accepted"] == true)
                || crate::launch_gate::accepted_commit(w, key).is_some()
            {
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
