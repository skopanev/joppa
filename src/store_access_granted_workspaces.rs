//! Part of `store_access`: `granted_workspaces` and the items after it, moved from store_access.rs unchanged.
use super::*;

/// The workspaces that owners granted to this email or to this Google hd.
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

/// The owner grants a workspace to an email. A refused request stores nothing.
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
    let workspace = super::grants::owner_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    conn.execute(
        "INSERT INTO joppa.email_grants(workspace,email,created_by) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
        &[&workspace, &email, &actor],
    )
    .await?;
    Ok(json!({"workspace": workspace, "email": email}))
}

/// The owner grants a workspace to every email in a domain. A refused request stores nothing.
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
    let workspace = super::grants::owner_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    conn.execute(
        "INSERT INTO joppa.domain_grants(workspace,domain,created_by) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
        &[&workspace, &domain, &actor],
    )
    .await?;
    Ok(json!({"workspace": workspace, "domain": domain}))
}

/// The workspaces that this actor owns, each with its granted emails.
pub async fn list_email_grants(pool: &Pool, actor: &str) -> Result<Value> {
    list_grants(pool, actor, "email_grants", "email", "emails").await
}

/// The workspaces that this actor owns, each with its granted email domains.
pub async fn list_domain_grants(pool: &Pool, actor: &str) -> Result<Value> {
    list_grants(pool, actor, "domain_grants", "domain", "domains").await
}

/// The owner removes an email grant. A missing grant changes nothing.
pub async fn revoke_email(pool: &Pool, workspace: &str, actor: &str, email: &str) -> Result<Value> {
    let email = email.trim().to_ascii_lowercase();
    let workspace = super::grants::owner_of(pool, workspace, actor).await?;
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

/// The owner removes a domain grant. A missing grant changes nothing.
pub async fn revoke_domain(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    domain: &str,
) -> Result<Value> {
    let domain = domain.trim().to_ascii_lowercase();
    let workspace = super::grants::owner_of(pool, workspace, actor).await?;
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

/// The owner removes a member and the email grant of that member's Google account, so
/// the member loses access at the next request. A domain grant still admits its domain.
pub async fn remove_member(pool: &Pool, workspace: &str, actor: &str, id: &str) -> Result<Value> {
    let workspace = super::grants::owner_of(pool, workspace, actor).await?;
    ensure!(id != actor, "workspace owner cannot be removed");
    // The grant goes first: a failed journal write then leaves a member without a grant,
    // never a removed member whose grant admits them again.
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.email_grants g USING joppa.google_identities i
             WHERE g.workspace=$1 AND i.actor=$2 AND g.email=lower(i.email)",
            &[&workspace, &id],
        )
        .await?;
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

/// The workspaces that this actor owns, each with its people and its pending grants, in one read.
/// A person shows as "name <email>", as the email, or as the member ID when no Google identity is
/// bound. Agent members are not people, so the list omits them. An email grant of a person who
/// is already a member is not listed again.
pub async fn list_owned_access(pool: &Pool, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let items: Vec<Value> = conn
        .query(
            "SELECT COALESCE(b.first_view->>'workspace_id',s.workspace), COALESCE(b.first_view->>'title',s.workspace),
                    COALESCE((SELECT array_agg(COALESCE(NULLIF(i.display_name,'')||' <'||i.email||'>', i.email, m.key) ORDER BY m.key)
                              FROM jsonb_each_text(b.first_view->'members') m
                              LEFT JOIN joppa.google_identities i ON i.actor=m.key
                              WHERE m.value='human'), '{}'),
                    COALESCE((SELECT array_agg(m.key ORDER BY m.key)
                              FROM jsonb_each_text(b.first_view->'members') m
                              WHERE m.value='human'), '{}'),
                    s.value->>'admin',
                    COALESCE((SELECT array_agg(e.email ORDER BY e.email) FROM joppa.email_grants e WHERE e.workspace=s.workspace
                              AND NOT EXISTS(SELECT 1 FROM jsonb_each_text(b.first_view->'members') m
                                             JOIN joppa.google_identities i ON i.actor=m.key WHERE lower(i.email)=e.email)), '{}'),
                    COALESCE((SELECT array_agg(d.domain ORDER BY d.domain) FROM joppa.domain_grants d WHERE d.workspace=s.workspace), '{}')
             FROM joppa.state_entries s
             LEFT JOIN joppa.board_cache b ON b.workspace=s.workspace
             WHERE s.section='meta' AND s.key='' AND s.value->>'admin'=$1 AND s.value->'deleted_at' IS NULL
             ORDER BY 2,1",
            &[&actor],
        )
        .await?
        .iter()
        .map(|r| {
            // Each member has its ID, so the owner can remove it; the owner row has no removal.
            let labels: Vec<String> = r.get(2);
            let ids: Vec<String> = r.get(3);
            let owner: String = r.get(4);
            let members: Vec<Value> = ids
                .iter()
                .zip(&labels)
                .map(|(id, label)| json!({"id": id, "label": label, "owner": *id == owner}))
                .collect();
            json!({"id": r.get::<_, String>(0), "title": r.get::<_, String>(1),
                "people": labels, "members": members, "emails": r.get::<_, Vec<String>>(5),
                "domains": r.get::<_, Vec<String>>(6)})
        })
        .collect();
    Ok(json!({"items": items}))
}
