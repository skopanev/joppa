//! The workspace timeline: one entry for each journal event, in journal order.
//! This module maps a stored command to its kind of change, its object and its reason.
//! The store reads the events and the object revisions.
use serde_json::Value;

/// The kind of change for each journal operation. A new operation needs one line here.
/// An operation that is not in the table has the kind "other".
const CHANGES: &[(&str, &[&str])] = &[
    (
        "save",
        &[
            "save_object",
            "revise",
            "domain",
            "capability",
            "consumer",
            "project",
            "edit_domain",
            "edit_capability",
            "edit_consumer",
            "describe",
            "decide_note",
            "set_note_status",
        ],
    ),
    ("archive", &["archive_object"]),
    ("restore", &["restore_object"]),
    ("comment", &["comment", "note", "withdraw_note"]),
    ("file", &["attach", "detach"]),
    ("tests", &["set_ac_tests"]),
    ("readiness", &["assess"]),
    ("priority", &["set_priority"]),
    (
        "task",
        &[
            "task",
            "claim_task",
            "claim_next_task",
            "amend_task",
            "release_task",
            "block_task",
            "unblock_task",
            "complete_task",
            "review_task",
            "retire_task",
            "heartbeat_task",
        ],
    ),
    ("report", &["record_report"]),
    (
        "planning",
        &["claim_planning", "finish_planning", "release_planning"],
    ),
    (
        "member",
        &["member", "remove_member", "enroll_member", "enroll_service"],
    ),
    ("schema", &["set_tree_schema", "reorder_tree"]),
    (
        "workspace",
        &["workspace", "rename_workspace", "delete_workspace"],
    ),
];

/// The kind of change of one stored command. A save that changes only the parent is a
/// move, but only the object revisions show that: the store applies `moved`.
pub fn change(command: &Value) -> &'static str {
    let op = command["op"].as_str().unwrap_or_default();
    if op == "set_domain_archived" {
        return if command["archived"] == true {
            "archive"
        } else {
            "restore"
        };
    }
    CHANGES
        .iter()
        .find(|(_, ops)| ops.contains(&op))
        .map_or("other", |(change, _)| change)
}

/// A save is a move when the new revision has the data and links of the previous
/// revision and a different parent.
pub fn moved(saved: &Value, previous: &Value) -> bool {
    saved["parent"] != previous["parent"]
        && saved["data"] == previous["data"]
        && saved["links"] == previous["links"]
}

/// The address of a stored report. Tree addresses are kind:id, so this cannot equal one.
pub fn report_address(report: i64) -> String {
    format!("report/{report}")
}

/// The object that the command changes, before alias resolution. None for a command
/// on the whole workspace, such as a member or a schema change.
pub fn address(command: &Value) -> Option<String> {
    let text = |field: &str| command[field].as_str().filter(|s| !s.is_empty());
    let op = command["op"].as_str().unwrap_or_default();
    match op {
        "save_object" => Some(format!("{}:{}", text("kind")?, text("id")?)),
        "record_report" => command["report"].as_i64().map(report_address),
        "assess" | "claim_planning" | "finish_planning" | "release_planning" => {
            text("ac").map(crate::objects::criterion_address)
        }
        "comment" => text("ac")
            .map(crate::objects::criterion_address)
            .or_else(|| text("req").map(crate::objects::requirement_address)),
        "revise" => text("req").map(crate::objects::requirement_address),
        "domain"
        | "capability"
        | "consumer"
        | "edit_domain"
        | "edit_capability"
        | "edit_consumer"
        | "set_domain_archived" => Some(format!(
            "{}:{}",
            op.trim_start_matches("edit_")
                .trim_start_matches("set_")
                .trim_end_matches("_archived"),
            text("id")?
        )),
        _ if change(command) == "task" => Some(crate::tasks::address(
            text("req").unwrap_or_default(),
            text("id")?,
        )),
        // The other process operations of a requirement name it in req.
        _ => text("target")
            .map(str::to_owned)
            .or_else(|| text("req").map(crate::objects::requirement_address)),
    }
}

/// The reason that the command states. A command without a reason gives an empty text.
pub fn reason(command: &Value) -> String {
    if command["op"] == "record_report" {
        let commit = command["commit"].as_str().unwrap_or_default();
        return format!(
            "{} {} on {}: {} tests",
            if command["repeat"] == true {
                "Repeated test report of commit"
            } else {
                "Test report of commit"
            },
            &commit[..commit.len().min(12)],
            command["branch"].as_str().unwrap_or_default(),
            command["tests"].as_u64().unwrap_or_default()
        );
    }
    [
        &command["reason"],
        &command["assessment"]["reason"],
        &command["evidence"],
    ]
    .into_iter()
    .find_map(Value::as_str)
    .unwrap_or_default()
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_listed_change_has_its_kind() {
        for (command, kind) in [
            (json!({"op":"save_object"}), "save"),
            (json!({"op":"archive_object"}), "archive"),
            (json!({"op":"restore_object"}), "restore"),
            (json!({"op":"note"}), "comment"),
            (json!({"op":"attach"}), "file"),
            (json!({"op":"set_ac_tests"}), "tests"),
            (json!({"op":"assess"}), "readiness"),
            (json!({"op":"set_priority"}), "priority"),
            (json!({"op":"claim_task"}), "task"),
            (json!({"op":"record_report"}), "report"),
            (
                json!({"op":"set_domain_archived","archived":true}),
                "archive",
            ),
            (json!({"op":"plan"}), "other"),
        ] {
            assert_eq!(change(&command), kind, "{command}");
        }
    }

    #[test]
    fn each_command_names_its_object() {
        for (command, expected) in [
            (
                json!({"op":"save_object","kind":"spec","id":"one"}),
                Some("spec:one"),
            ),
            (
                json!({"op":"archive_object","target":"note:n"}),
                Some("note:n"),
            ),
            (
                json!({"op":"assess","req":"spec:one","ac":"criterion:a"}),
                Some("criterion:a"),
            ),
            (
                json!({"op":"claim_task","req":"","id":"work:t"}),
                Some("work:t"),
            ),
            (
                json!({"op":"claim_task","req":"r","id":"t"}),
                Some("task:r/t"),
            ),
            (json!({"op":"record_report","report":7}), Some("report/7")),
            (json!({"op":"edit_domain","id":"d"}), Some("domain:d")),
            (
                json!({"op":"set_domain_archived","id":"d"}),
                Some("domain:d"),
            ),
            (json!({"op":"confirm","req":"spec:one"}), Some("spec:one")),
            (
                json!({"op":"set_priority","target":"spec:one"}),
                Some("spec:one"),
            ),
            (json!({"op":"member","id":"bot"}), None),
        ] {
            assert_eq!(address(&command).as_deref(), expected, "{command}");
        }
    }
}
