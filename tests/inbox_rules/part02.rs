// Part 2 of tests/inbox_rules.rs, included there, so the test names stay unchanged.

#[test]
fn only_a_person_rejects_or_marks_a_duplicate() {
    // person-closes-person-ac1, ac2, ac3
    let mut w = world();
    let command = decide(&w, "a", "rejected", json!({}));
    refused(
        &mut w,
        "agent",
        command,
        "only a person decides on a signal from a person",
    );
    let command = decide(&w, "b", "duplicate", json!({"duplicate_of":"note:a"}));
    refused(
        &mut w,
        "agent",
        command,
        "only a person decides on a signal from a person",
    );
    assert_eq!(
        data(&w, "a")["status"],
        "unprocessed",
        "the refused decision leaves the signal open"
    );
    w.ok("alex", decide(&w, "a", "rejected", json!({})));
    assert_eq!(
        (
            data(&w, "a")["decision"]["outcome"].as_str(),
            data(&w, "a")["decision"]["actor"].as_str()
        ),
        (Some("rejected"), Some("alex"))
    );
}

#[test]
fn an_account_without_membership_cannot_add_a_signal() {
    // signal-in-one-inbox-ac3
    let mut w = world();
    let command = json!({"op":"save_object","kind":"note","id":"eve","revision":0,"parent":null,"data":{"title":"Signal eve","owner":"owner","body":"Hi","source":{"author":"eve"}},"links":[],"reason":"Intake"});
    refused(&mut w, "eve", command, "not a workspace member");
}

#[test]
fn only_the_owner_redacts_a_fragment() {
    // owner-redacts-secret-ac2, ac3
    let mut w = world();
    let command =
        json!({"op":"redact_object","target":"note:a","fragment":"Refunds","reason":"private"});
    refused(&mut w, "agent", command.clone(), "workspace owner required");
    refused(&mut w, "alex", command.clone(), "workspace owner required");
    assert_eq!(data(&w, "a")["body"], "Refunds are slow");
    w.ok("owner", command);
    assert_eq!(data(&w, "a")["body"], "[redacted] are slow");
}
