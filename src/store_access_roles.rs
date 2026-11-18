//! Part of `store_access`: the roles of the people of a workspace. A grant enrolls its person
//! with a role, an Editor changes a role in Settings, and a Read-only member writes nothing.
use super::*;
use std::collections::BTreeMap;

/// The refusal of a write of a Read-only member. The web answers it with 403.
#[derive(Debug)]
pub struct ReadOnly;

impl std::fmt::Display for ReadOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", commands::READ_ONLY)
    }
}

impl std::error::Error for ReadOnly {}

/// Add the role of a grant to `roles`. human wins over viewer.
pub(crate) fn add_role(roles: &mut BTreeMap<String, String>, workspace: String, role: &str) {
    let kept = roles.entry(workspace).or_insert_with(|| role.to_owned());
    if role == "human" {
        *kept = role.to_owned();
    }
}

/// Refuse the actor when it is a Read-only member of the workspace (an internal ID).
pub(crate) async fn refuse_viewer(
    client: &impl GenericClient,
    workspace: &str,
    actor: &str,
) -> Result<()> {
    let viewer: bool = client
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2 AND value #>> '{}' = 'viewer')",
            &[&workspace, &actor],
        )
        .await?
        .get(0);
    if viewer {
        return Err(ReadOnly.into());
    }
    Ok(())
}

/// Refuse a write of a Read-only member of the workspace `name`. An unknown workspace gives
/// no refusal here: the access check of the write refuses it.
pub async fn check_writer(pool: &Pool, name: &str, actor: &str) -> Result<()> {
    let conn = pool.get().await?;
    let Ok(workspace) = resolve_workspace(&**conn, name).await else {
        return Ok(());
    };
    refuse_viewer(&**conn, &workspace, actor).await
}

/// Refuse the actor unless it is a human member (Editor) of the workspace: a Read-only member
/// gets "a human member of the workspace is required" on the Settings routes of the Editors.
pub async fn require_human(pool: &Pool, workspace: &str, actor: &str) -> Result<()> {
    human_member_of(pool, workspace, actor).await.map(|_| ())
}

/// Called only after verifying Google and resolving the private access policy. Enrollment leaves
/// an explicit system event, never a forged member action.
pub async fn enroll_roles(
    pool: &Pool,
    actor: &str,
    google_sub: &str,
    email: &str,
    hosted_domain: Option<&str>,
    roles: &BTreeMap<String, String>,
) -> Result<()> {
    commands::identifier(actor)?;
    ensure!(
        actor != "joppa-access" && !roles.is_empty(),
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
    join(&*tx, actor, roles).await?;
    tx.commit().await?;
    Ok(())
}

/// Add the actor to these workspaces with their roles, each with the enrollment event. A
/// present member keeps its role. The caller commits, so a failure leaves no partial membership.
pub(crate) async fn join(
    tx: &impl GenericClient,
    actor: &str,
    roles: &BTreeMap<String, String>,
) -> Result<()> {
    let mut normalized = BTreeMap::new();
    for (name, role) in roles {
        add_role(&mut normalized, resolve_workspace(tx, name).await?, role);
    }
    // BTreeMap order prevents opposite lock ordering on concurrent enrollment.
    for (workspace, role) in &normalized {
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
        if let Some(present) = state.members.get(actor) {
            ensure!(
                ["human", commands::VIEWER].contains(&present.as_str()),
                "Google cannot claim a service identity"
            );
            continue;
        }
        let id = actor.to_owned();
        let command = if role == commands::VIEWER {
            Command::EnrollViewer { id }
        } else {
            Command::EnrollMember { id }
        };
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
            tx,
            workspace,
            &state,
            &events,
            position as u64,
            Some(&previous),
        )
        .await?;
        // The enrollment reaches no object, so the search builder only moves its position.
        // Without it the search rows lag, and the next write rebuilds every object.
        let at = (base_position, position as u64);
        super::search_pending::add(tx, workspace, Some(&BTreeSet::new()), at).await?;
    }
    Ok(())
}

/// A human member sets the role of a member, or of an invited email, from Settings. A member
/// change also gives the email grant of its Google account the new role, in the same write.
pub async fn set_role(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    member: Option<&str>,
    email: Option<&str>,
    role: &str,
) -> Result<Value> {
    if ![commands::VIEWER, "human"].contains(&role) {
        return Err(commands::ClientField("role is viewer or human").into());
    }
    let (workspace, _) = human_member_of(pool, workspace, actor).await?;
    match (member, email) {
        (Some(id), None) => {
            let command = Command::SetRole {
                id: id.into(),
                role: role.into(),
            };
            let request = uuid::Uuid::new_v4().to_string();
            super::execute(pool, &workspace, actor, &request, &command).await?;
            Ok(json!({"workspace": workspace, "member": id, "role": role}))
        }
        (None, Some(email)) => {
            let email = email.trim().to_ascii_lowercase();
            let changed = pool
                .get()
                .await?
                .execute(
                    "UPDATE joppa.email_grants SET role=$3 WHERE workspace=$1 AND email=$2",
                    &[&workspace, &email, &role],
                )
                .await?;
            ensure!(changed > 0, "unknown email grant");
            Ok(json!({"workspace": workspace, "email": email, "role": role}))
        }
        _ => Err(commands::ClientField("name one member or email").into()),
    }
}
