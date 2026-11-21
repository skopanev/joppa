//! Part of `store_access`: `access_denied` and the items after it, moved from store_access.rs unchanged.
use super::*;

#[derive(Debug)]
pub struct AccessDenied;

impl std::fmt::Display for AccessDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Workspace not found")
    }
}

impl std::error::Error for AccessDenied {}

pub const MEMBERSHIP: &str =
    "SELECT workspace FROM joppa.state_entries WHERE section='members' AND key=$1";
/// The workspaces where the actor ($1) is a human member: they list in Settings with their
/// people and grants.
pub(crate) const HUMAN_MEMBERSHIP: &str = "SELECT workspace FROM joppa.state_entries WHERE section='members' AND key=$1 AND value #>> '{}' = 'human'";
/// Member grants for a lowercase email ($1) and a Google hd ($2), each with its role: its email
/// grants, and the domain grants that equal the hd as viewer. Without hd, no domain grant matches.
pub(crate) const GRANTED: &str = "SELECT workspace, role FROM joppa.email_grants WHERE email=$1
     UNION ALL SELECT workspace, 'viewer' FROM joppa.domain_grants WHERE domain=lower($2::text)";
/// The workspaces that the actor ($1) created. The creator has no grant, so this keeps its
/// access path. It gives no right: membership still decides, so a removed creator loses access.
pub(crate) const CREATED: &str = "SELECT workspace FROM joppa.state_entries WHERE section='meta' AND key='' AND value->>'admin'=$1 AND value->'deleted_at' IS NULL";

pub async fn scope(client: &impl GenericClient, actor: &str) -> Result<Option<BTreeSet<String>>> {
    let identity = client
        .query_opt(
            "SELECT email,hosted_domain FROM joppa.google_identities WHERE actor=$1",
            &[&actor],
        )
        .await?;
    let (email, hosted_domain): (String, Option<String>) = if let Some(row) = identity {
        (row.get(0), row.get(1))
    } else {
        // Human API keys obey explicit rules before their first Google sign-in.
        // Unbound service identities still require journal membership.
        let Some(users_path) = std::env::var_os("JOPPA_LOGIN_USERS_FILE") else {
            return Ok(None);
        };
        if std::env::var_os("JOPPA_ACCESS_RULES_FILE").is_none() {
            return Ok(None);
        }
        let users: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(users_path).map_err(|_| AccessDenied)?)
                .map_err(|_| AccessDenied)?;
        let Some((email, _)) = users.into_iter().find(|(_, id)| id == actor) else {
            return Ok(None);
        };
        (email, None)
    };
    let mut allowed = BTreeSet::new();
    if let Some(path) = std::env::var_os("JOPPA_ACCESS_RULES_FILE") {
        let rules =
            crate::access::Rules::read(std::path::Path::new(&path)).map_err(|_| AccessDenied)?;
        for name in rules.resolve(&email, hosted_domain.as_deref()) {
            allowed.insert(resolve_workspace(client, &name).await?);
        }
    }
    // Member email and domain grants add workspaces, also without a rules file.
    // Only a bound Google identity has an hd, so only it gets domain grants.
    for row in client
        .query(GRANTED, &[&email.to_ascii_lowercase(), &hosted_domain])
        .await?
    {
        allowed.insert(row.get(0));
    }
    // The creator keeps a workspace that the rules do not name. Membership is still checked.
    for row in client.query(CREATED, &[&actor]).await? {
        allowed.insert(row.get(0));
    }
    Ok(Some(allowed))
}

pub async fn check(
    client: &impl GenericClient,
    workspace: &str,
    actor: &str,
    creating: bool,
) -> Result<()> {
    let human_only = match crate::write_key::current() {
        Some(key_ws) if creating || workspace != key_ws => return Err(AccessDenied.into()),
        Some(_) => true,
        None => false,
    };
    if scope(client, actor)
        .await?
        .is_some_and(|allowed| !allowed.contains(workspace))
    {
        return Err(AccessDenied.into());
    }
    let sql = if human_only {
        "SELECT EXISTS(SELECT 1 FROM joppa.workspaces WHERE id=$1), EXISTS(
         SELECT 1 FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2 AND value='\"human\"'::jsonb)"
    } else {
        "SELECT EXISTS(SELECT 1 FROM joppa.workspaces WHERE id=$1), EXISTS(
         SELECT 1 FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2)"
    };
    let row = client.query_one(sql, &[&workspace, &actor]).await?;
    let exists: bool = row.get(0);
    let member: bool = row.get(1);
    if member || (creating && !exists) {
        Ok(())
    } else {
        Err(AccessDenied.into())
    }
}

/// Called only after verifying Google and resolving the private access policy. Each workspace
/// enrolls the person as a human member (store_access_roles.rs gives other roles).
pub async fn enroll(
    pool: &Pool,
    actor: &str,
    google_sub: &str,
    email: &str,
    hosted_domain: Option<&str>,
    workspaces: &BTreeSet<String>,
) -> Result<()> {
    let roles = workspaces
        .iter()
        .map(|w| (w.clone(), "human".to_owned()))
        .collect();
    enroll_roles(pool, actor, google_sub, email, hosted_domain, &roles).await
}

/// Save the name from a verified profile without changing the bound identity.
pub async fn update_identity_name(
    pool: &Pool,
    actor: &str,
    google_sub: &str,
    name: &str,
) -> Result<()> {
    let name = name.trim();
    ensure!(
        !name.is_empty() && name.chars().count() <= 200 && !name.chars().any(char::is_control),
        "Invalid profile name"
    );
    let conn = pool.get().await?;
    conn.execute(
        "UPDATE joppa.google_identities SET display_name=$3 WHERE actor=$1 AND google_sub=$2",
        &[&actor, &google_sub, &name],
    )
    .await?;
    Ok(())
}
