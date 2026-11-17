//! Part of `commands`: the roles of the people of a workspace. A human member (Editor) reads,
//! writes and administers the workspace. A viewer (Read-only) only reads it.
use super::*;

/// The role of a Read-only member.
pub const VIEWER: &str = "viewer";

/// The refusal of a write of a Read-only member.
pub const READ_ONLY: &str = "a Read-only member cannot write";

/// Apply `set_role`: a human member sets the role of a person, while another human member stays.
pub(super) fn apply_set_role(w: &mut Workspace, actor: &str, id: &str, role: &str) -> Result<()> {
    human_member(w, actor)?;
    ensure!([VIEWER, "human"].contains(&role), "role is viewer or human");
    let current = w.members.get(id).context("unknown member")?;
    ensure!(
        [VIEWER, "human"].contains(&current.as_str()),
        "only a person has the role viewer or human"
    );
    if role == VIEWER {
        ensure!(
            w.members.iter().any(|(m, r)| m != id && r == "human"),
            "the last human member cannot become Read-only"
        );
    }
    w.members.insert(id.into(), role.into());
    Ok(())
}
