//! The process facts and the signal of an object, and the run summary of a requirement.
use super::*;

/// The run summary of a requirement revision: one entry for each present criterion.
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
    // An archived criterion is not part of the requirement.
    if let Some(acs) = summary["acs"].as_object_mut() {
        acs.retain(|id, _| {
            w.objects
                .get(&criterion_address(id))
                .is_none_or(|o| o.current().present)
        });
    }
    if blocked || !w.objects.is_empty() && !work_ready(w, &requirement_address(req)) {
        summary["ready_for_acceptance"] = json!(false);
        summary["accepted"] = json!(false);
    }
    summary
}

/// The process facts of an object. After migrate_test_rules the test results decide a
/// criterion, so a criterion and a requirement give no facts of the removed check runs.
pub fn process(w: &Workspace, key: &str) -> Value {
    let mut p = run_process(w, key);
    let role = w
        .objects
        .get(resolve_address(w, key))
        .and_then(|o| schema(w).types.get(&o.kind).map(|t| t.role.clone()));
    // The revision stays: assess names it. An MCP read of a criterion gives it in assessment.
    let removed: &[&str] = match role.as_deref() {
        Some("ac") => &["status", "pending_run"],
        Some("requirement") => &["summary"],
        _ => &[],
    };
    if w.test_rules_migrated
        && let Some(map) = p.as_object_mut()
    {
        for field in removed {
            map.remove(*field);
        }
        // Only the verdict of a finished run shows; no run gives no verification.
        if !removed.is_empty()
            && !matches!(
                map.get("verification").and_then(Value::as_str),
                Some("pass" | "fail")
            )
        {
            map.remove("verification");
        }
    }
    p
}

/// The process facts with the facts of the check runs, which the signal and the parent
/// gate read.
pub fn run_process(w: &Workspace, key: &str) -> Value {
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
    let p = run_process(w, key);
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
