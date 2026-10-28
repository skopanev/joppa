//! Agent tokens: the owner creates one per agent and workspace and can revoke it.
//! A token always names its agent, so an agent never acts as a person.
use super::*;

fn token_hash(token: &str) -> String {
    crate::digest_hex::sha256_hex(token.as_bytes())
}

/// The one reply to a write with a read grant, on MCP and HTTP.
pub const READ_ONLY_REFUSAL: &str = "read-only token";

/// What a token may do. Write is the default; a read grant permits only reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Write,
    Read,
}
impl Access {
    /// A missing value means write. Any value other than write or read is refused.
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("write") => Ok(Self::Write),
            Some("read") => Ok(Self::Read),
            Some(_) => Err(commands::ClientField("access must be write or read").into()),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Read => "read",
        }
    }
}

/// An authenticated agent token: its agent, its workspace and its access grant.
#[derive(Clone, Debug)]
pub struct TokenGrant {
    pub agent: String,
    pub workspace: String,
    pub access: Access,
}

/// The grant behind a token: Ok(None) for an unknown token, an error for a revoked one.
pub async fn token_actor(pool: &Pool, token: &str) -> Result<Option<TokenGrant>> {
    let conn = pool.get().await?;
    let Some(row) = conn
        .query_opt(
            "SELECT t.agent, EXISTS(SELECT 1 FROM joppa.agent_token_revocations r WHERE r.token_id=t.id),
                    t.workspace, t.access
             FROM joppa.agent_tokens t WHERE t.token_hash=$1",
            &[&token_hash(token)],
        )
        .await?
    else {
        return Ok(None);
    };
    ensure!(!row.get::<_, bool>(1), "token revoked");
    Ok(Some(TokenGrant {
        agent: row.get(0),
        workspace: row.get(2),
        access: Access::parse(Some(row.get::<_, &str>(3)))?,
    }))
}

pub(super) async fn owner_of(pool: &Pool, workspace: &str, actor: &str) -> Result<String> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    let (state, _) = state_rows::current(&**conn, &workspace).await?;
    ensure!(state.deleted_at.is_none(), "Workspace not found");
    ensure!(state.admin == actor, "workspace owner required");
    Ok(workspace)
}

/// The owner creates a token for an agent. The agent enrolls through enroll_service.
/// A missing access means write. The raw token is returned once.
pub async fn create_agent_token(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    agent: &str,
    access: Option<&str>,
) -> Result<Value> {
    // Checked before enrollment, so a refused request stores nothing.
    let access = Access::parse(access)?;
    commands::identifier(agent)?;
    let workspace = owner_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    let (state, _) = state_rows::current(&**conn, &workspace).await?;
    if let Some(role) = state.members.get(agent) {
        ensure!(role == "agent", "{agent} is a person, not an agent");
    } else {
        drop(conn);
        super::write::enroll_service(pool, &workspace, actor, agent, "agent").await?;
    }
    let token = format!(
        "jpt_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let conn = pool.get().await?;
    let id: i64 = conn
        .query_one(
            "INSERT INTO joppa.agent_tokens(token_hash,agent,workspace,created_by,access) VALUES($1,$2,$3,$4,$5) RETURNING id",
            &[&token_hash(&token), &agent, &workspace, &actor, &access.as_str()],
        )
        .await?
        .get(0);
    Ok(json!({"id": id, "agent": agent, "access": access.as_str(), "token": token}))
}

/// The owner revokes a token. The next call with it is refused.
pub async fn revoke_agent_token(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    id: i64,
) -> Result<Value> {
    let workspace = owner_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    let agent: String = conn
        .query_opt(
            "SELECT agent FROM joppa.agent_tokens WHERE id=$1 AND workspace=$2",
            &[&id, &workspace],
        )
        .await?
        .context("unknown agent token")?
        .get(0);
    conn.execute(
        "INSERT INTO joppa.agent_token_revocations(token_id,revoked_by) VALUES($1,$2) ON CONFLICT DO NOTHING",
        &[&id, &actor],
    )
    .await?;
    Ok(json!({"id": id, "agent": agent, "revoked": true}))
}

/// Each item holds only the name, access, status and dates; never the token or its hash.
pub async fn list_agent_tokens(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let workspace = owner_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    let items: Vec<Value> = conn
        .query(
            "SELECT t.id, t.agent, t.access, t.created_at::text, r.revoked_at::text
             FROM joppa.agent_tokens t
             LEFT JOIN joppa.agent_token_revocations r ON r.token_id=t.id
             WHERE t.workspace=$1 ORDER BY t.id",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| {
            let revoked_at: Option<String> = r.get(4);
            json!({"id": r.get::<_, i64>(0), "agent": r.get::<_, String>(1), "access": r.get::<_, String>(2), "revoked": revoked_at.is_some(), "created_at": r.get::<_, String>(3), "revoked_at": revoked_at})
        })
        .collect();
    Ok(json!({"items": items}))
}
