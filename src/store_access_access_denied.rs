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
/// Owner grants for a lowercase email ($1) and a Google hd ($2): its email grants, and the
/// domain grants that equal the hd. Without hd, no domain grant matches.
pub(crate) const GRANTED: &str = "SELECT workspace FROM joppa.email_grants WHERE email=$1
     UNION SELECT workspace FROM joppa.domain_grants WHERE domain=lower($2::text)";
pub(crate) const OWNED: &str = "SELECT workspace FROM joppa.state_entries WHERE section='meta' AND key='' AND value->>'admin'=$1 AND value->'deleted_at' IS NULL";

/// Resolve types for the authors of the records that this read returns.
pub(crate) async fn actor_types(
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
pub(crate) async fn join(
    tx: &impl GenericClient,
    actor: &str,
    workspaces: &BTreeSet<String>,
) -> Result<()> {
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
        // A rule for a deleted workspace admits nobody. A deletion removes the workspace,
        // so the rule then names an unknown workspace.
        if state.admin.is_empty() || state.deleted_at.is_some() {
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
