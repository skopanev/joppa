//! Agent tokens: the owner creates one per agent and workspace and can revoke it.
//! A token always names its agent, so an agent never acts as a person.
use super::*;
use sha2::{Digest, Sha256};

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

/// The agent behind a token: Ok(None) for an unknown token, an error for a revoked one.
pub async fn token_actor(pool: &Pool, token: &str) -> Result<Option<String>> {
    let conn = pool.get().await?;
    let Some(row) = conn
        .query_opt(
            "SELECT t.agent, EXISTS(SELECT 1 FROM joppa.agent_token_revocations r WHERE r.token_id=t.id)
             FROM joppa.agent_tokens t WHERE t.token_hash=$1",
            &[&token_hash(token)],
        )
        .await?
    else {
        return Ok(None);
    };
    ensure!(!row.get::<_, bool>(1), "token revoked");
    Ok(Some(row.get(0)))
}

async fn owner_of(pool: &Pool, workspace: &str, actor: &str) -> Result<String> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    let (state, _) = state_rows::current(&**conn, &workspace).await?;
    ensure!(state.admin == actor, "workspace owner required");
    Ok(workspace)
}

/// The owner creates a token for an agent. The agent becomes a member with role agent.
/// The raw token is returned once.
pub async fn create_agent_token(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    agent: &str,
) -> Result<Value> {
    commands::identifier(agent)?;
    let workspace = owner_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    let (state, _) = state_rows::current(&**conn, &workspace).await?;
    if let Some(role) = state.members.get(agent) {
        ensure!(role == "agent", "{agent} is a person, not an agent");
    } else {
        drop(conn);
        let enroll = Command::Member {
            id: agent.into(),
            role: "agent".into(),
        };
        execute(
            pool,
            &workspace,
            actor,
            &format!("agent-token-{}", uuid::Uuid::new_v4()),
            &enroll,
        )
        .await?;
    }
    let token = format!(
        "jpt_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let conn = pool.get().await?;
    let id: i64 = conn
        .query_one(
            "INSERT INTO joppa.agent_tokens(token_hash,agent,workspace,created_by) VALUES($1,$2,$3,$4) RETURNING id",
            &[&token_hash(&token), &agent, &workspace, &actor],
        )
        .await?
        .get(0);
    Ok(json!({"id": id, "agent": agent, "token": token}))
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

pub async fn list_agent_tokens(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let workspace = owner_of(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    let items: Vec<Value> = conn
        .query(
            "SELECT t.id, t.agent, t.created_by, t.created_at::text,
                    EXISTS(SELECT 1 FROM joppa.agent_token_revocations r WHERE r.token_id=t.id)
             FROM joppa.agent_tokens t WHERE t.workspace=$1 ORDER BY t.id",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| json!({"id": r.get::<_, i64>(0), "agent": r.get::<_, String>(1), "created_by": r.get::<_, String>(2), "created_at": r.get::<_, String>(3), "revoked": r.get::<_, bool>(4)}))
        .collect();
    Ok(json!({"items": items}))
}
