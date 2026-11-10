use super::*;

/// One revision of a requirement at a journal position, with a body.
fn revision(number: u32, position: u64, body: &str) -> ObjectRevision {
    serde_json::from_value(
        json!({"revision": number, "schema_revision": 1, "parent": null,
        "data": {"title": "R", "body": body}, "relations": {}, "actor": "bot", "at": "",
        "position": position, "reason": "r", "present": true}),
    )
    .unwrap()
}

#[test]
fn forced_bumps_of_earlier_operations_give_the_current_revision() {
    // Revision 5 before the write at base position 10, then two forced bumps at 11 and 12.
    let revisions = [
        revision(5, 4, "a"),
        revision(6, 11, "a"),
        revision(7, 12, "a"),
    ];
    let forced = |position: u64| position > 10;
    assert_eq!(caught_up(&revisions, 5, forced), Some(7));
    assert_eq!(caught_up(&revisions, 6, forced), Some(7));
    // A revision that the write did not force stays stale.
    assert_eq!(caught_up(&revisions, 5, |p| p == 12), None);
}

#[test]
fn a_changed_revision_or_an_unknown_sent_revision_stays_stale() {
    let changed = [
        revision(8, 4, "a"),
        revision(9, 11, "b"),
        revision(10, 12, "b"),
    ];
    assert_eq!(caught_up(&changed, 8, |_| true), None);
    assert_eq!(caught_up(&changed, 9, |_| true), Some(10));
    // The sent revision is not in memory, so no rule can compare it.
    assert_eq!(caught_up(&changed[1..], 8, |_| true), None);
    // The sent revision is current or above it: nothing to catch up.
    assert_eq!(caught_up(&changed, 10, |_| true), None);
    assert_eq!(caught_up(&changed, 11, |_| true), None);
}
