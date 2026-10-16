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
/// Owner grants for a lowercase email ($1) and a Google hd ($2): its email grants, and the
/// domain grants that equal the hd. Without hd, no domain grant matches.
const GRANTED: &str = "SELECT workspace FROM joppa.email_grants WHERE email=$1
     UNION SELECT workspace FROM joppa.domain_grants WHERE domain=lower($2::text)";
const OWNED: &str = "SELECT workspace FROM joppa.state_entries WHERE section='meta' AND key='' AND value->>'admin'=$1 AND value->'deleted_at' IS NULL";

/// Resolve types for the authors of the records that this read returns.
pub(super) async fn actor_types(
    client: &impl GenericClient,
    workspace: &str,
    actors: &[String],
) -> Result<std::collections::BTreeMap<String, &'static str>> {
    if actors.is_empty() {
        return Ok(Default::default());
    }
    let rows = client.query(
        "SELECT a.actor,m.value#>>'{}', (SELECT value->>'admin' FROM joppa.state_entries WHERE workspace=$1 AND section='meta' AND key='') FROM unnest($2::text[]) a(actor) LEFT JOIN joppa.state_entries m ON m.workspace=$1 AND m.section='members' AND m.key=a.actor",
        &[&workspace, &actors],
    ).await?;
    let mut state = Workspace {
        admin: rows[0].get::<_, Option<String>>(2).unwrap_or_default(),
        ..Default::default()
    };
    for row in &rows {
        if let Some(role) = row.get::<_, Option<String>>(1) {
            state.members.insert(row.get(0), role);
        }
    }
    Ok(actors
        .iter()
        .map(|actor| (actor.clone(), crate::inbox::actor_type(&state, actor)))
        .collect())
}

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
    // Owner email and domain grants add workspaces, also without a rules file.
    // Only a bound Google identity has an hd, so only it gets domain grants.
    for row in client
        .query(GRANTED, &[&email.to_ascii_lowercase(), &hosted_domain])
        .await?
    {
        allowed.insert(row.get(0));
    }
    // The owner keeps a workspace that the rules do not name. Membership is still checked.
    for row in client.query(OWNED, &[&actor]).await? {
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
    // A first sign-in without hd leaves NULL. A later verified hd fills it for read scope.
    tx.execute(
        "UPDATE joppa.google_identities SET hosted_domain=$2 WHERE actor=$1 AND hosted_domain IS NULL AND $2::text IS NOT NULL",
        &[&actor, &hosted_domain],
    )
    .await?;
    join(&*tx, actor, workspaces).await?;
    tx.commit().await?;
    Ok(())
}

/// Add the actor to these workspaces as human, each with the enrollment event. The caller
/// commits, so a failure leaves no partial membership.
async fn join(tx: &impl GenericClient, actor: &str, workspaces: &BTreeSet<String>) -> Result<()> {
    let mut normalized = BTreeSet::new();
    for name in workspaces {
        normalized.insert(resolve_workspace(tx, name).await?);
    }
    // BTreeSet order prevents opposite lock ordering on concurrent enrollment.
    for workspace in &normalized {
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
            &[workspace],
        )
        .await?;
        let (mut state, base_position) = state_rows::current(tx, workspace).await?;
        let previous = state.clone();
        ensure!(
            !state.admin.is_empty(),
            "Access rule refers to an unknown workspace"
        );
        // A grant to a deleted workspace admits nobody.
        if state.deleted_at.is_some() {
            continue;
        }
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
            actor_type: crate::inbox::actor_type(&state, "joppa-access").into(),
            recorded_at: at,
            command,
        }];
        super::board::project_current(
            tx,
            workspace,
            &state,
            &events,
            position as u64,
            Some(&previous),
        )
        .await?;
    }
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
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.email_grants g USING joppa.google_identities i
             WHERE g.workspace=$1 AND i.actor=$2 AND g.email=lower(i.email)",
            &[&workspace, &id],
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

async fn list_grants(
    pool: &Pool,
    actor: &str,
    table: &str,
    column: &str,
    key: &str,
) -> Result<Value> {
    let conn = pool.get().await?;
    let items: Vec<Value> = conn
        .query(
            &format!("SELECT COALESCE(b.first_view->>'workspace_id',s.workspace), COALESCE(b.first_view->>'title',s.workspace),
                    COALESCE(array_agg(g.{column} ORDER BY g.{column}) FILTER (WHERE g.{column} IS NOT NULL), '{{}}')
             FROM joppa.state_entries s
             LEFT JOIN joppa.board_cache b ON b.workspace=s.workspace
             LEFT JOIN joppa.{table} g ON g.workspace=s.workspace
             WHERE s.section='meta' AND s.key='' AND s.value->>'admin'=$1 AND s.value->'deleted_at' IS NULL
             GROUP BY 1,2 ORDER BY 2,1"),
            &[&actor],
        )
        .await?
        .iter()
        .map(|r| {
            let mut item = json!({"id": r.get::<_, String>(0), "title": r.get::<_, String>(1)});
            item[key] = json!(r.get::<_, Vec<String>>(2));
            item
        })
        .collect();
    Ok(json!({"items": items}))
}
