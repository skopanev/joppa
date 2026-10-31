use anyhow::Result;
use joppa::{commands, model::Workspace, objects};
use serde_json::{Value, json};

fn at(w: &mut Workspace, actor: &str, value: Value, minute: i64) -> Result<()> {
    let time = chrono::DateTime::parse_from_rfc3339("2026-10-11T00:00:00Z")?
        + chrono::Duration::minutes(minute);
    let migration = value["op"]
        .as_str()
        .is_some_and(|op| op.starts_with("migrate_"));
    let command = serde_json::from_value(value)?;
    let mut copy = w.clone();
    let position = w
        .objects
        .values()
        .map(|o| o.current().position)
        .max()
        .unwrap_or(0)
        + 1;
    if migration {
        commands::apply_replay(&mut copy, actor, &command, position, &time.to_rfc3339())?;
    } else {
        commands::apply(&mut copy, actor, &command, position, &time.to_rfc3339())?;
    }
    *w = copy;
    Ok(())
}
fn fixture(timed: bool) -> Result<Workspace> {
    let mut w = Workspace::default();
    at(
        &mut w,
        "owner",
        json!({"op":"workspace","title":"Claims"}),
        0,
    )?;
    for actor in ["bot", "other"] {
        at(
            &mut w,
            "owner",
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
            0,
        )?;
    }
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
    ] {
        at(&mut w, "owner", json!({"op":op}), 0)?;
    }
    if timed {
        at(&mut w, "owner", json!({"op":"migrate_task_claims"}), 0)?;
    }
    let revision = objects::schema(&w).revision;
    at(
        &mut w,
        "owner",
        json!({"op":"set_tree_schema","revision":revision,"types":{"job":{"title":"Jobs","role":"task","status":"none","root":true,"parents":["*"]}},"reason":"Configured work"}),
        0,
    )?;
    at(
        &mut w,
        "owner",
        json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement","owner":"owner","body":"Implement and verify","purpose":"implementation"},"reason":"Create work"}),
        0,
    )?;
    at(
        &mut w,
        "bot",
        json!({"op":"claim_task","id":"job:one","revision":1}),
        0,
    )?;
    Ok(w)
}

// [ac:heartbeat-return-ac1]
#[test]
fn elapsed_time_never_returns_claimed_work() -> Result<()> {
    for minute in [31, 120, 1440] {
        let mut w = fixture(true)?;
        let before = serde_json::to_value(&w)?;
        let refused = at(
            &mut w,
            "other",
            json!({"op":"claim_task","id":"job:one","revision":2}),
            minute,
        );
        assert_eq!(refused.unwrap_err().to_string(), "task is already claimed");
        assert_eq!(serde_json::to_value(&w)?, before);
        assert_eq!(objects::process(&w, "job:one")["status"], "in_progress");
        at(
            &mut w,
            "bot",
            json!({"op":"complete_task","commit":"c0ffee0","id":"job:one","revision":2,"evidence":"Verified work"}),
            minute,
        )?;
        assert!(w.task_process["job:one"].done.is_some());
    }
    Ok(())
}

// [ac:heartbeat-return-ac2]
#[test]
fn new_heartbeat_requests_fail_without_state_changes() -> Result<()> {
    let mut w = fixture(true)?;
    let before = serde_json::to_value(&w)?;
    let refused = at(
        &mut w,
        "bot",
        json!({"op":"heartbeat_task","id":"job:one","revision":2}),
        5,
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "heartbeat_task is no longer supported"
    );
    assert_eq!(serde_json::to_value(&w)?, before);
    assert!(!include_str!("../docs/api.md").contains("heartbeat_task"));
    Ok(())
}

fn replay_at(
    w: &mut Workspace,
    actor: &str,
    value: Value,
    minute: i64,
    position: u64,
) -> Result<()> {
    let time = chrono::DateTime::parse_from_rfc3339("2026-10-11T00:00:00Z")?
        + chrono::Duration::minutes(minute);
    commands::apply_replay(
        w,
        actor,
        &serde_json::from_value(value)?,
        position,
        &time.to_rfc3339(),
    )
}

// [ac:claims-persist-replay]
#[test]
fn journal_cutover_preserves_old_reclaims_and_delayed_completion() -> Result<()> {
    let mut w = fixture(true)?;
    replay_at(
        &mut w,
        "bot",
        json!({"op":"heartbeat_task","id":"job:one","revision":2}),
        5,
        20,
    )?;
    replay_at(
        &mut w,
        "other",
        json!({"op":"claim_task","id":"job:one","revision":2}),
        40,
        21,
    )?;
    assert_eq!(w.task_process["job:one"].assignee.as_deref(), Some("other"));
    replay_at(
        &mut w,
        "owner",
        json!({"op":"migrate_persistent_claims"}),
        41,
        22,
    )?;
    let mut live = w.clone();
    let command = json!({"op":"complete_task","commit":"c0ffee0","id":"job:one","revision":3,"evidence":"Verified after one day"});
    let time = "2026-10-12T00:00:00Z";
    commands::apply(
        &mut live,
        "other",
        &serde_json::from_value(command.clone())?,
        23,
        time,
    )?;
    commands::apply_replay(&mut w, "other", &serde_json::from_value(command)?, 23, time)?;
    assert_eq!(serde_json::to_value(w)?, serde_json::to_value(live)?);
    Ok(())
}

// [ac:owner-releases-claim-ac1]
// [ac:old-agent-locked-out-ac1]
// [ac:returned-work-kept-ac1]
#[test]
fn only_the_holder_or_workspace_owner_can_release_and_old_holder_cannot_complete() -> Result<()> {
    let mut w = fixture(true)?;
    let before = serde_json::to_value(&w)?;
    let error = at(
        &mut w,
        "other",
        json!({"op":"release_task","id":"job:one","revision":2,"reason":"Worker absent"}),
        10,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "workspace owner required");
    assert_eq!(serde_json::to_value(&w)?, before);
    let document = w.objects["job:one"].current().data.clone();
    at(
        &mut w,
        "owner",
        json!({"op":"release_task","id":"job:one","revision":2,"reason":"Worker absent"}),
        10,
    )?;
    assert_eq!(w.objects["job:one"].current().data, document);
    let before = serde_json::to_value(&w)?;
    let command = json!({"op":"complete_task","commit":"c0ffee0","id":"job:one","revision":3,"evidence":"Late completion"});
    assert_eq!(
        at(&mut w, "bot", command, 11).unwrap_err().to_string(),
        "Task is not claimed unfinished work"
    );
    assert_eq!(serde_json::to_value(&w)?, before);
    at(
        &mut w,
        "other",
        json!({"op":"claim_task","id":"job:one","revision":3}),
        11,
    )?;
    let before = serde_json::to_value(&w)?;
    for actor in ["bot", "owner"] {
        let command = json!({"op":"complete_task","commit":"c0ffee0","id":"job:one","revision":4,"evidence":"Late completion"});
        assert_eq!(
            at(&mut w, actor, command, 12).unwrap_err().to_string(),
            "Task is not claimed unfinished work"
        );
        assert_eq!(serde_json::to_value(&w)?, before);
    }
    Ok(())
}
