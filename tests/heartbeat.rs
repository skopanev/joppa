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
            json!({"op":"member","id":actor,"role":"agent"}),
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
        json!({"op":"set_tree_schema","revision":revision,"types":{"job":{"title":"Jobs","role":"task","root":true,"parents":["*"]}},"reason":"Configured work"}),
        0,
    )?;
    at(
        &mut w,
        "owner",
        json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement","owner":"owner","contract":"Implement and verify","purpose":"implementation"},"reason":"Create work"}),
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

#[test]
fn five_minute_heartbeats_preserve_the_claim_at_minute_31() -> Result<()> {
    let mut w = fixture(true)?;
    let document = w.objects["job:one"].current().clone();
    for minute in (5..=30).step_by(5) {
        at(
            &mut w,
            "bot",
            json!({"op":"heartbeat_task","id":"job:one","revision":2}),
            minute,
        )?;
        assert_eq!(
            serde_json::to_value(w.objects["job:one"].current())?,
            serde_json::to_value(&document)?
        );
        let claim = w.task_process["job:one"].claim.as_ref().unwrap();
        assert_eq!(claim.deadline_ms - claim.heartbeat_at_ms, 30 * 60_000);
    }
    assert!(
        at(
            &mut w,
            "other",
            json!({"op":"claim_task","id":"job:one","revision":2}),
            31
        )
        .is_err()
    );
    assert!(
        at(
            &mut w,
            "bot",
            json!({"op":"heartbeat_task","id":"job:one","revision":2}),
            60
        )
        .is_err()
    );
    assert!(
        at(
            &mut w,
            "bot",
            json!({"op":"complete_task","id":"job:one","revision":2,"evidence":"Expired"}),
            60
        )
        .is_err()
    );
    at(
        &mut w,
        "other",
        json!({"op":"claim_task","id":"job:one","revision":2}),
        60,
    )?;
    assert_eq!(w.task_process["job:one"].assignee.as_deref(), Some("other"));
    assert_eq!(w.objects["job:one"].current().revision, 3);
    Ok(())
}

#[test]
fn an_old_claim_gets_30_minutes_at_migration() -> Result<()> {
    let mut w = fixture(false)?;
    assert!(w.task_process["job:one"].claim.is_none());
    let document = serde_json::to_value(&w.objects["job:one"])?;
    at(&mut w, "owner", json!({"op":"migrate_task_claims"}), 100)?;
    let migration_time = chrono::DateTime::parse_from_rfc3339("2026-10-11T00:00:00Z")?
        .timestamp_millis()
        + 100 * 60_000;
    let claim = w.task_process["job:one"].claim.clone().unwrap();
    assert_eq!(claim.heartbeat_at_ms, migration_time);
    assert_eq!(claim.deadline_ms, migration_time + 30 * 60_000);
    assert_eq!(serde_json::to_value(&w.objects["job:one"])?, document);
    at(&mut w, "owner", json!({"op":"migrate_task_claims"}), 101)?;
    assert_eq!(
        w.task_process["job:one"]
            .claim
            .as_ref()
            .unwrap()
            .deadline_ms,
        claim.deadline_ms
    );
    let mut expired = w.clone();
    for op in ["heartbeat_task", "complete_task"] {
        let mut command = json!({"op":op,"id":"job:one","revision":2});
        if op == "complete_task" {
            command["evidence"] = json!("Expired claim");
        }
        assert!(at(&mut expired, "bot", command, 130).is_err());
    }
    at(
        &mut expired,
        "other",
        json!({"op":"claim_task","id":"job:one","revision":2}),
        130,
    )?;
    assert_eq!(
        expired.task_process["job:one"].assignee.as_deref(),
        Some("other")
    );
    assert!(
        at(
            &mut w,
            "other",
            json!({"op":"claim_task","id":"job:one","revision":2}),
            101
        )
        .is_err()
    );
    at(
        &mut w,
        "bot",
        json!({"op":"heartbeat_task","id":"job:one","revision":2}),
        101,
    )?;
    assert!(
        at(
            &mut w,
            "other",
            json!({"op":"claim_task","id":"job:one","revision":2}),
            130
        )
        .is_err()
    );
    at(
        &mut w,
        "other",
        json!({"op":"claim_task","id":"job:one","revision":2}),
        131,
    )?;
    Ok(())
}
