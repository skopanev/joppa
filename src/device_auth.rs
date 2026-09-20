//! A short-lived browser approval exchanges a device secret for a durable CLI key.
use anyhow::{Result, ensure};
use deadpool_postgres::Pool;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

struct Attempt {
    secret_hash: String,
    expires: Instant,
    actor: Option<String>,
}
#[derive(Clone, Default)]
pub struct Devices(Arc<Mutex<BTreeMap<String, Attempt>>>);
impl Devices {
    pub fn start(&self) -> Result<Value> {
        let mut all = self.0.lock().unwrap();
        all.retain(|_, a| a.expires > Instant::now());
        ensure!(
            all.len() < 100,
            "Too many connection attempts. Try again later."
        );
        let code = loop {
            let raw = random()[..12].to_ascii_uppercase();
            let code = format!("{}-{}-{}", &raw[..4], &raw[4..8], &raw[8..]);
            if !all.contains_key(&code) {
                break code;
            }
        };
        let secret = random();
        all.insert(
            code.clone(),
            Attempt {
                secret_hash: hash(&secret),
                expires: Instant::now() + Duration::from_secs(600),
                actor: None,
            },
        );
        Ok(json!({"code":code,"device_secret":secret,"expires_in":600,"interval":3}))
    }
    pub fn approve(&self, code: &str, actor: &str) -> Result<()> {
        let mut all = self.0.lock().unwrap();
        let a = all
            .get_mut(code)
            .ok_or_else(|| anyhow::anyhow!("Invalid or expired connection code"))?;
        ensure!(
            a.expires > Instant::now(),
            "Connection expired. Run joppa login again."
        );
        ensure!(a.actor.is_none(), "Connection already approved");
        a.actor = Some(actor.into());
        Ok(())
    }
    pub fn poll(&self, code: &str, secret: &str) -> Result<Option<String>> {
        let mut all = self.0.lock().unwrap();
        let a = all
            .get(code)
            .ok_or_else(|| anyhow::anyhow!("Invalid or expired connection"))?;
        ensure!(
            a.secret_hash == hash(secret) && a.expires > Instant::now(),
            "Invalid or expired connection"
        );
        if a.actor.is_none() {
            return Ok(None);
        }
        Ok(all.remove(code).unwrap().actor)
    }
}
pub async fn issue(pool: &Pool, actor: &str) -> Result<String> {
    let token = format!("jp_{}", random());
    pool.get()
        .await?
        .execute(
            "INSERT INTO joppa.cli_credentials(token_hash,actor) VALUES($1,$2)",
            &[&hash(&token), &actor],
        )
        .await?;
    Ok(token)
}
pub async fn actor(pool: &Pool, token: &str) -> Result<Option<String>> {
    if token.len() != 67
        || !token.starts_with("jp_")
        || !token[3..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Ok(None);
    }
    Ok(pool.get().await?.query_opt("SELECT actor FROM joppa.cli_credentials c WHERE token_hash=$1 AND expires_at>clock_timestamp() AND NOT EXISTS (SELECT 1 FROM joppa.revoked_cli_credentials r WHERE r.token_hash=c.token_hash)", &[&hash(token)]).await?.map(|r| r.get(0)))
}
pub async fn revoke(pool: &Pool, token: &str) -> Result<()> {
    pool.get().await?.execute("INSERT INTO joppa.revoked_cli_credentials(token_hash) SELECT token_hash FROM joppa.cli_credentials WHERE token_hash=$1 ON CONFLICT DO NOTHING", &[&hash(token)]).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_is_secret_bound_single_use_and_expires() {
        let d = Devices::default();
        let v = d.start().unwrap();
        let code = v["code"].as_str().unwrap();
        let secret = v["device_secret"].as_str().unwrap();
        assert!(d.poll(code, "wrong").is_err());
        assert!(d.poll(code, secret).unwrap().is_none());
        d.approve(code, "owner").unwrap();
        assert!(d.approve(code, "attacker").is_err());
        assert!(d.poll(code, "wrong").is_err());
        assert_eq!(d.poll(code, secret).unwrap().as_deref(), Some("owner"));
        assert!(d.poll(code, secret).is_err());
        let v = d.start().unwrap();
        let code = v["code"].as_str().unwrap();
        d.0.lock().unwrap().get_mut(code).unwrap().expires =
            Instant::now() - Duration::from_secs(1);
        assert!(d.approve(code, "owner").is_err());
        assert!(d.poll(code, v["device_secret"].as_str().unwrap()).is_err());
    }
}
