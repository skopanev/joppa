//! Durable browser sessions. Only token hashes are stored; revocation is append-only.
use anyhow::Result;
use deadpool_postgres::Pool;
use sha2::{Digest, Sha256};

pub const LIFETIME_SECONDS: u64 = 43_200;
fn hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
pub async fn create(pool: &Pool, token: &str, actor: &str) -> Result<()> {
    pool.get().await?.execute(
        "INSERT INTO joppa.browser_sessions(token_hash, actor, expires_at) VALUES($1,$2,clock_timestamp()+interval '12 hours')",
        &[&hash(token), &actor],
    ).await?;
    Ok(())
}
pub async fn actor(pool: &Pool, token: &str) -> Result<Option<String>> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    Ok(pool.get().await?.query_opt(
        "SELECT actor FROM joppa.browser_sessions s WHERE token_hash=$1 AND expires_at>clock_timestamp() AND NOT EXISTS (SELECT 1 FROM joppa.revoked_sessions r WHERE r.token_hash=s.token_hash)",
        &[&hash(token)],
    ).await?.map(|row| row.get(0)))
}
pub async fn revoke(pool: &Pool, token: &str) -> Result<()> {
    pool.get().await?.execute(
        "INSERT INTO joppa.revoked_sessions(token_hash) SELECT token_hash FROM joppa.browser_sessions WHERE token_hash=$1 ON CONFLICT DO NOTHING",
        &[&hash(token)],
    ).await?;
    Ok(())
}
