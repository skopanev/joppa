//! The rights of the human members: each human member has the rights that the creator had,
//! an agent keeps its refusals, and no refusal names a workspace owner.
use super::*;

/// bob creates W. alex is a human member who did not create W, and bot is an agent.
fn workspace() -> Workspace {
    let mut w = Workspace::default();
    let create = Command::Workspace { title: "W".into() };
    apply_inner(&mut w, "bob", &create, 1, "").unwrap();
    w.members.insert("alex".into(), "human".into());
    w.members.insert("bot".into(), "agent".into());
    w
}

fn refusal(w: &mut Workspace, actor: &str, command: &Command) -> String {
    let error = apply_inner(w, actor, command, 2, "")
        .unwrap_err()
        .to_string();
    assert!(!error.contains("owner"), "{error}");
    error
}

fn title(text: &str) -> Command {
    Command::UpdateWorkspace(UpdateWorkspaceOp {
        title: Some(text.into()),
        ..Default::default()
    })
}

#[test]
fn a_human_member_who_did_not_create_the_workspace_runs_the_member_commands() {
    let mut w = workspace();
    let carol = Command::Member {
        id: "carol".into(),
        role: "human".into(),
    };
    let remove = Command::RemoveMember {
        id: "carol".into(),
        reason: "left".into(),
    };
    let delete = Command::DeleteWorkspace {
        reason: "done".into(),
    };
    let human = "a human member of the workspace is required";
    assert_eq!(
        refusal(&mut w, "bot", &title("Z")),
        "a human member updates the workspace"
    );
    assert_eq!(refusal(&mut w, "bot", &carol), human);
    assert_eq!(refusal(&mut w, "bot", &delete), human);
    assert_eq!(w.title, "W");
    apply_inner(&mut w, "alex", &title("Y"), 3, "").unwrap();
    apply_inner(&mut w, "alex", &carol, 4, "").unwrap();
    assert_eq!(refusal(&mut w, "bot", &remove), human);
    apply_inner(&mut w, "alex", &remove, 5, "").unwrap();
    assert_eq!(w.title, "Y");
    assert!(!w.members.contains_key("carol"));
    apply_inner(&mut w, "alex", &delete, 6, "").unwrap();
    assert!(w.members.is_empty());
}

#[test]
fn a_human_member_removes_the_creator_and_the_last_human_member_stays() {
    let mut w = workspace();
    let remove = |id: &str| Command::RemoveMember {
        id: id.into(),
        reason: "left".into(),
    };
    apply_inner(&mut w, "alex", &remove("bob"), 2, "").unwrap();
    let last = "the last human member cannot be removed";
    assert_eq!(refusal(&mut w, "alex", &remove("alex")), last);
    assert_eq!(w.members.get("alex").map(String::as_str), Some("human"));
    // An agent never counts as a human member.
    assert_eq!(
        refusal(&mut w, "bot", &remove("alex")),
        "a human member of the workspace is required"
    );
}

#[test]
fn the_enroll_service_refusal_names_no_owner() {
    let mut w = workspace();
    let enroll = Command::EnrollService {
        id: "runner".into(),
        role: "runner".into(),
    };
    refusal(&mut w, "alex", &enroll);
    apply_inner(&mut w, "bob", &enroll, 3, "").unwrap();
    assert_eq!(w.members.get("runner").map(String::as_str), Some("runner"));
}

#[test]
fn a_new_domain_is_confirmed_by_a_human_member_only_from_the_epoch_on() {
    let domain = |id: &str| {
        Command::Domain(DomainOp {
            id: id.into(),
            title: id.into(),
            owner: "alex".into(),
            description: String::new(),
        })
    };
    let mut w = workspace();
    apply_inner(&mut w, "alex", &domain("live"), 2, "").unwrap();
    apply_inner(&mut w, "bot", &domain("draft"), 3, "").unwrap();
    assert!(w.domains["live"].confirmed);
    assert!(!w.domains["draft"].confirmed);
    // A replay before the epoch keeps the older rule: only the creator confirmed.
    w.replay_epochs.insert(HUMAN_MEMBER_RIGHTS.into(), 10);
    w.replaying = true;
    apply_inner(&mut w, "alex", &domain("old"), 4, "").unwrap();
    apply_inner(&mut w, "bob", &domain("creator"), 5, "").unwrap();
    apply_inner(&mut w, "alex", &domain("new"), 10, "").unwrap();
    assert!(!w.domains["old"].confirmed);
    assert!(w.domains["creator"].confirmed);
    assert!(w.domains["new"].confirmed);
}
