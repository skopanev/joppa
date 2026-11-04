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

/// The process facts of a node, the same for every type: the verdict of its checks, its
/// readiness mark, its acceptance, the address of its comments and the status of an inbox
/// signal.
pub fn process(w: &Workspace, key: &str) -> Value {
    let key = resolve_address(w, key);
    let Some(o) = w.objects.get(key) else {
        return json!({});
    };
    let mut p = json!({"annotation_address": annotation_address(w, key)});
    if input_type(w, &o.kind) {
        p["status"] = json!(o.current().data["status"].as_str().unwrap_or("unprocessed"));
    }
    let verdict = verification(w, key);
    if verdict != "unverified" {
        p["verification"] = json!(verdict);
    }
    if let Some(a) = assessment(w, key) {
        p["assessment"] = a;
    }
    let acceptance = crate::acceptance::current(w, key);
    if !acceptance.is_null() {
        p["acceptance"] = acceptance;
    }
    p
}

/// The readiness mark of a node, as `assess` sets it: ready, or waiting with a reason. Its
/// position is the expected_position of the next assess.
pub fn assessment(w: &Workspace, key: &str) -> Option<Value> {
    let a = w.signal_assessments.get(resolve_address(w, key))?;
    let state = if a.ready { "ready" } else { "waiting" };
    Some(json!({"state": state, "reason": a.reason, "actor": a.actor, "position": a.position}))
}

/// The process tone of a node. A status rule of its type decides the tone that a read shows.
pub fn signal(w: &Workspace, key: &str) -> Value {
    let Some(o) = w.objects.get(resolve_address(w, key)) else {
        return json!({});
    };
    let p = process(w, key);
    let (tone, label) = if !o.current().present {
        ("grey", "Archived")
    } else if !work_ready(w, key) {
        ("yellow", "Dependency is not ready")
    } else if p["assessment"]["state"] == "waiting" {
        ("yellow", "Waits for an answer")
    } else if p["status"] == "processed" {
        ("grey", "Input processed")
    } else if p["status"] == "unprocessed" {
        ("yellow", "Input awaits processing")
    } else {
        match p["verification"].as_str() {
            Some("fail") => ("red", "Checks failed"),
            Some("pass") => ("green", "Checks passed"),
            _ => ("grey", "Current document"),
        }
    };
    let mut signal = json!({"tone":tone,"label":label,"at":o.current().at});
    if p["assessment"].is_object() {
        signal["assessment"] = p["assessment"].clone();
    }
    signal
}
