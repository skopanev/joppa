//! Item IDs start with a slug of the workspace title, such as billing-team-R-0A1B2C3D4E.
//! The server derives the prefix on create and on each rename. An explicit reference_prefix
//! of rename_workspace stays until the next rename without one. The event is an ordinary
//! rename_workspace with the derived prefix, so the database trigger keeps every older item
//! ID as an alias of its object.
use super::*;

pub const REASON: &str = "Item IDs start with a slug of the workspace title";

/// Lowercase Latin letters and digits, with one hyphen between words, at most 40 bytes.
/// A title without Latin letters or digits gives an empty slug.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_end_matches('-').chars().take(40).collect();
    out.trim_end_matches('-').to_owned()
}

/// The prefix that the workspace must have now, or None when it keeps its prefix: an explicit
/// prefix, or a title without a slug. Another workspace never shares a prefix: the second one
/// gets -2, the third -3, and so on.
pub(super) async fn wanted(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
) -> Result<Option<String>> {
    if state.reference_prefix.is_some() && !state.prefix_from_title {
        return Ok(None);
    }
    let base = slug(&state.title);
    if base.is_empty() {
        return Ok(None);
    }
    let current = state.reference_prefix.as_deref().unwrap_or(workspace);
    for n in 1..1000 {
        let candidate = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        if candidate == current {
            return Ok(None);
        }
        let taken: bool = client.query_one("SELECT EXISTS(SELECT 1 FROM joppa.board_cache WHERE workspace<>$1 AND COALESCE(state->>'reference_prefix',workspace)=$2)
            OR EXISTS(SELECT 1 FROM joppa.item_ids WHERE workspace<>$1 AND namespace=$2) OR EXISTS(SELECT 1 FROM joppa.workspaces WHERE id=$2 AND id<>$1)", &[&workspace, &candidate]).await?.get(0);
        if !taken {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}
