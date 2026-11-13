//! Part of `store_access`: `granted_workspaces` and the items after it, moved from store_access.rs unchanged.
use super::*;

/// The workspaces that members granted to this email or to this Google hd.
pub async fn granted_workspaces(
    pool: &Pool,
    email: &str,
    hd: Option<&str>,
) -> Result<BTreeSet<String>> {
    let conn = pool.get().await?;
    Ok(conn
        .query(GRANTED, &[&email.to_ascii_lowercase(), &hd])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect())
}

/// A browser session joins each workspace granted to its email since sign-in: the bound
/// Google email, else the configured login email. Domain grants use the hd of this session,
/// and only for a bound identity. It binds no identity. Without an email or a new grant,
/// nothing is written.
pub async fn enroll_granted(
    pool: &Pool,
    actor: &str,
    configured: Option<&str>,
    session_hd: Option<&str>,
) -> Result<()> {
    let conn = pool.get().await?;
    let bound: Option<String> = conn
        .query_opt(
            "SELECT email FROM joppa.google_identities WHERE actor=$1",
            &[&actor],
        )
        .await?
        .map(|row| row.get(0));
    let hd = bound.as_ref().and(session_hd);
    let Some(email) = bound.or_else(|| configured.map(str::to_owned)) else {
        return Ok(());
    };
    let joined: BTreeSet<String> = conn
        .query(MEMBERSHIP, &[&actor])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    drop(conn);
    let missing: BTreeSet<String> = granted_workspaces(pool, &email.to_ascii_lowercase(), hd)
        .await?
        .difference(&joined)
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    commands::identifier(actor)?;
    ensure!(actor != "joppa-access", "No workspace access");
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    join(&*tx, actor, &missing).await?;
    tx.commit().await?;
    Ok(())
}

/// A human member grants a workspace to an email. A refused request stores nothing.
pub async fn grant_email(pool: &Pool, workspace: &str, actor: &str, email: &str) -> Result<Value> {
    let email = email.trim().to_ascii_lowercase();
    if email.len() > 254
        || !email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
        || email.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(commands::ClientField("a valid email is required").into());
    }
    let (workspace, _) = human_member_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    conn.execute(
        "INSERT INTO joppa.email_grants(workspace,email,created_by) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
        &[&workspace, &email, &actor],
    )
    .await?;
    Ok(json!({"workspace": workspace, "email": email}))
}

/// A human member grants a workspace to every email in a domain. A refused request stores
/// nothing.
pub async fn grant_domain(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    domain: &str,
) -> Result<Value> {
    let domain = domain.to_ascii_lowercase();
    if domain.len() > 253
        || !domain.contains('.')
        || domain
            .chars()
            .any(|c| c == '@' || c.is_whitespace() || c.is_control())
    {
        return Err(commands::ClientField("a valid email domain is required").into());
    }
    let (workspace, _) = human_member_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    conn.execute(
        "INSERT INTO joppa.domain_grants(workspace,domain,created_by) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
        &[&workspace, &domain, &actor],
    )
    .await?;
    Ok(json!({"workspace": workspace, "domain": domain}))
}

/// The workspaces where this actor is a human member, each with its granted emails.
pub async fn list_email_grants(pool: &Pool, actor: &str) -> Result<Value> {
    list_grants(pool, actor, "email_grants", "email", "emails").await
}

/// The workspaces where this actor is a human member, each with its granted email domains.
pub async fn list_domain_grants(pool: &Pool, actor: &str) -> Result<Value> {
    list_grants(pool, actor, "domain_grants", "domain", "domains").await
}

/// A human member removes an email grant. A missing grant changes nothing.
pub async fn revoke_email(pool: &Pool, workspace: &str, actor: &str, email: &str) -> Result<Value> {
    let email = email.trim().to_ascii_lowercase();
    let (workspace, _) = human_member_of(pool, workspace, actor).await?;
    let removed = pool
        .get()
        .await?
        .execute(
            "DELETE FROM joppa.email_grants WHERE workspace=$1 AND email=$2",
            &[&workspace, &email],
        )
        .await?;
    Ok(json!({"workspace": workspace, "email": email, "removed": removed > 0}))
}

/// A human member removes a domain grant. A missing grant changes nothing.
pub async fn revoke_domain(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    domain: &str,
) -> Result<Value> {
    let domain = domain.trim().to_ascii_lowercase();
    let (workspace, _) = human_member_of(pool, workspace, actor).await?;
    let removed = pool
        .get()
        .await?
        .execute(
            "DELETE FROM joppa.domain_grants WHERE workspace=$1 AND domain=$2",
            &[&workspace, &domain],
        )
        .await?;
    Ok(json!({"workspace": workspace, "domain": domain, "removed": removed > 0}))
}

/// A human member removes a member and the email grant of that member's Google account, so
/// the member loses access at the next request. A domain grant still admits its domain. The
/// last human member stays, and so does its grant.
pub async fn remove_member(pool: &Pool, workspace: &str, actor: &str, id: &str) -> Result<Value> {
    let (workspace, state) = human_member_of(pool, workspace, actor).await?;
    commands::keeps_human_member(&state, id)?;
    // The write removes the email grant in the transaction of the event (store_write_operations).
    let command = Command::RemoveMember {
        id: id.into(),
        reason: "Removed in Settings".into(),
    };
    super::execute(
        pool,
        &workspace,
        actor,
        &uuid::Uuid::new_v4().to_string(),
        &command,
    )
    .await?;
    Ok(json!({"workspace": workspace, "member": id, "removed": true}))
}

/// The workspace and its current state when `actor` is a human member of it. Any other member
/// gets the human member rule, and an outsider gets "Workspace not found".
pub(crate) async fn human_member_of(
    pool: &Pool,
    workspace: &str,
    actor: &str,
) -> Result<(String, Workspace)> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let (state, _) = state_rows::current(&**conn, &workspace).await?;
    ensure!(state.deleted_at.is_none(), "Workspace not found");
    commands::human_member(&state, actor)?;
    Ok((workspace, state))
}
