// Part 2 of tests/inbox_rules.rs, included there, so the test names stay unchanged.

#[test]
fn an_account_without_membership_cannot_add_a_signal() {
    // signal-in-one-inbox-ac3
    let mut w = world();
    let command = json!({"op":"save_object","kind":"note","id":"eve","revision":0,"parent":null,"data":{"title":"Signal eve","owner":"owner","body":"Hi","source":{"author":"eve"}},"reason":"Intake"});
    refused(&mut w, "eve", command, "not a workspace member");
}

#[test]
fn a_human_member_who_did_not_create_the_workspace_redacts_a_fragment() {
    // redaction-owner-only: alex is a human member and did not create the workspace.
    let mut w = world();
    let command =
        json!({"op":"redact_object","target":"note:a","fragment":"Refunds","reason":"private"});
    refused(
        &mut w,
        "agent",
        command.clone(),
        "a human member of the workspace is required",
    );
    assert_eq!(data(&w, "a")["body"], "Refunds are slow");
    w.ok("alex", command);
    assert_eq!(data(&w, "a")["body"], "[redacted] are slow");
}
