use super::*;
use std::collections::BTreeSet;

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
    let path = std::env::var_os("JOPPA_ACCESS_RULES_FILE").ok_or(AccessDenied)?;
    let rules =
        crate::access::Rules::read(std::path::Path::new(&path)).map_err(|_| AccessDenied)?;
    let mut allowed = BTreeSet::new();
    for name in rules.resolve(&email, hosted_domain.as_deref()) {
        allowed.insert(resolve_workspace(client, &name).await?);
    }
    Ok(Some(allowed))
}

pub async fn check(
    client: &impl GenericClient,
    workspace: &str,
    actor: &str,
    creating: bool,
) -> Result<()> {
    if scope(client, actor)
        .await?
        .is_some_and(|allowed| !allowed.contains(workspace))
    {
        return Err(AccessDenied.into());
    }
    let row = client
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM joppa.workspaces WHERE id=$1), EXISTS(
         SELECT 1 FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2)",
            &[&workspace, &actor],
        )
        .await?;
    let exists: bool = row.get(0);
    let member: bool = row.get(1);
    if member || (creating && !exists) {
        Ok(())
    } else {
        Err(AccessDenied.into())
    }
}

/// Called only after verifying Google and resolving the private access policy.
/// Enrollment leaves an explicit system event, never a forged owner action.
pub async fn enroll(
    pool: &Pool,
    actor: &str,
    google_sub: &str,
    email: &str,
    hosted_domain: Option<&str>,
    workspaces: &BTreeSet<String>,
) -> Result<()> {
    commands::identifier(actor)?;
    ensure!(
        actor != "joppa-access" && !workspaces.is_empty(),
        "No workspace access"
    );
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("INSERT INTO joppa.google_identities(actor,google_sub,email,hosted_domain) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING", &[&actor,&google_sub,&email,&hosted_domain]).await?;
    let identity = tx
        .query_opt(
            "SELECT google_sub,email FROM joppa.google_identities WHERE actor=$1",
            &[&actor],
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("Identity already belongs to another account"))?;
    ensure!(
        identity.get::<_, String>(0) == google_sub && identity.get::<_, String>(1) == email,
        "Identity already belongs to another account"
    );
    let mut normalized = BTreeSet::new();
    for name in workspaces {
        normalized.insert(resolve_workspace(&*tx, name).await?);
    }
    // BTreeSet order prevents opposite lock ordering on concurrent enrollment.
    for workspace in &normalized {
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
            &[workspace],
        )
        .await?;
        let (mut state, base_position) = state_rows::current(&*tx, workspace).await?;
        let previous = state.clone();
        ensure!(
            !state.admin.is_empty(),
            "Access rule refers to an unknown workspace"
        );
        if let Some(role) = state.members.get(actor) {
            ensure!(role == "human", "Google cannot claim a service identity");
            continue;
        }
        let command = Command::EnrollMember { id: actor.into() };
        let position = base_position as i64 + 1;
        commands::apply(&mut state, "joppa-access", &command, position as u64, "")?;
        let at: String = tx.query_one("INSERT INTO joppa.events(workspace,position,request_id,actor,command) VALUES($1,$2,$3,'joppa-access',$4::text::jsonb) RETURNING recorded_at::text", &[workspace,&position,&format!("enroll-{actor}"),&serde_json::to_string(&command)?]).await?.get(0);
        let events = [Event {
            position: position as u64,
            actor: "joppa-access".into(),
            recorded_at: at,
            command,
        }];
        super::board::project_current(
            &*tx,
            workspace,
            &state,
            &events,
            position as u64,
            Some(&previous),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
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
