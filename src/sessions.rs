//! Durable browser sessions. Only token hashes are stored; revocation is append-only.
use anyhow::Result;
use deadpool_postgres::Pool;

/// Неделя от входа. Абсолютный срок: активность его не продлевает.
/// Двенадцати часов не хватало — вход утром, к вечеру страница уже
/// выкидывала на логин, и это читалось как поломка, а не как политика.
pub const LIFETIME_SECONDS: u64 = 604_800;
fn hash(token: &str) -> String {
    crate::digest_hex::sha256_hex(token.as_bytes())
}
/// The session keeps the verified Google hd of this sign-in, in lowercase. None means no hd.
pub async fn create(pool: &Pool, token: &str, actor: &str, hd: Option<&str>) -> Result<()> {
    let hd = hd.map(str::to_ascii_lowercase);
    pool.get().await?.execute(
        // Срок считается от одной константы, а не от литерала в SQL: два
        // места разъезжаются молча — кука живёт дольше строки в базе.
        "INSERT INTO joppa.browser_sessions(token_hash, actor, expires_at, hd) VALUES($1,$2,clock_timestamp()+make_interval(secs => $3),$4)",
        &[&hash(token), &actor, &(LIFETIME_SECONDS as f64), &hd],
    ).await?;
    Ok(())
}
/// The actor of a valid session and the hd of its sign-in.
pub async fn actor(pool: &Pool, token: &str) -> Result<Option<(String, Option<String>)>> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    Ok(pool.get().await?.query_opt(
        "SELECT actor,hd FROM joppa.browser_sessions s WHERE token_hash=$1 AND expires_at>clock_timestamp() AND NOT EXISTS (SELECT 1 FROM joppa.revoked_sessions r WHERE r.token_hash=s.token_hash)",
        &[&hash(token)],
    ).await?.map(|row| (row.get(0), row.get(1))))
}
pub async fn revoke(pool: &Pool, token: &str) -> Result<()> {
    pool.get().await?.execute(
        "INSERT INTO joppa.revoked_sessions(token_hash) SELECT token_hash FROM joppa.browser_sessions WHERE token_hash=$1 ON CONFLICT DO NOTHING",
        &[&hash(token)],
    ).await?;
    Ok(())
}
