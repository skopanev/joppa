//! Part of `history_entry`: the kind of change of each journal operation.

/// The kind of change for each journal operation. A new operation needs one line here.
/// An operation that is not in the table has the kind "other".
pub(super) const CHANGES: &[(&str, &[&str])] = &[
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
    ("results", &["record_report"]),
    (
        "planning",
        &["claim_planning", "finish_planning", "release_planning"],
    ),
    (
        "member",
        &[
            "member",
            "remove_member",
            "set_role",
            "enroll_member",
            "enroll_viewer",
            "enroll_service",
        ],
    ),
    ("schema", &["set_tree_schema", "reorder_tree"]),
    (
        "workspace",
        &[
            "workspace",
            "rename_workspace",
            "update_workspace",
            "delete_workspace",
            "remove_workspace_references",
        ],
    ),
];
