//! Joppa-owned Google sign-in attempts and browser sessions.
use crate::google_auth::{Google, Identity};
use anyhow::{Result, ensure};
use axum::http::{HeaderMap, header};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Pending {
    state: String,
    nonce: String,
    verifier: String,
    expires: Instant,
    processing: bool,
    result: Option<std::result::Result<String, String>>,
}
#[derive(Clone)]
pub struct Login {
    pub users: BTreeMap<String, String>,
    rules: Option<std::path::PathBuf>,
    google: Option<Google>,
    pending: Arc<Mutex<BTreeMap<String, Pending>>>,
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
impl Login {
    pub fn from_env() -> Result<Self> {
        // Keys are verified Google email addresses, never another product's user IDs.
        let users: BTreeMap<String, String> = match std::env::var("JOPPA_LOGIN_USERS_FILE") {
            Ok(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
            Err(_) => BTreeMap::new(),
        };
        for (email, actor) in &users {
            ensure!(
                email.contains('@'),
                "login users must be Google email addresses"
            );
            joppa::commands::identifier(actor)?;
            ensure!(actor != "joppa-access", "reserved system identity");
        }
        let normalized: BTreeMap<_, _> = users
            .iter()
            .map(|(email, actor)| (email.to_ascii_lowercase(), actor.clone()))
            .collect();
        ensure!(
            normalized.len() == users.len(),
            "duplicate email identities"
        );
        ensure!(
            users
                .values()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == users.len(),
            "each Google email must have its own actor"
        );
        let rules = std::env::var_os("JOPPA_ACCESS_RULES_FILE").map(std::path::PathBuf::from);
        if let Some(path) = &rules {
            joppa::access::Rules::read(path)?;
        }
        Ok(Self {
            users: normalized,
            rules,
            google: Google::from_env()?,
            pending: Default::default(),
        })
    }
    pub async fn start(&self, previous: Option<&str>) -> Result<(String, Value)> {
        let google = self.google.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Google sign-in is being configured for Joppa. Please try again later.")
        })?;
        ensure!(
            !self.users.is_empty() || self.rules.is_some(),
            "No Joppa accounts have been invited yet."
        );
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, p| p.expires > Instant::now());
        if let Some(id) = previous {
            pending.remove(id);
        }
        ensure!(
            pending.len() < 100,
            "Too many sign-in attempts. Please try again later."
        );
        let id = random();
        let p = Pending {
            state: random(),
            nonce: random(),
            verifier: random(),
            expires: Instant::now() + Duration::from_secs(300),
            processing: false,
            result: None,
        };
        let url = google.authorize(&p.state, &p.nonce, &p.verifier);
        pending.insert(id.clone(), p);
        Ok((id, json!({"verification_url":url,"expires_in":300})))
    }
    pub async fn callback(
        &self,
        pool: &deadpool_postgres::Pool,
        id: &str,
        state: &str,
        code: Option<&str>,
        cancelled: bool,
    ) -> Result<()> {
        let p = {
            let mut all = self.pending.lock().unwrap();
            let p = all
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("Sign-in expired. Please try again."))?;
            ensure!(
                p.expires > Instant::now()
                    && p.state == state
                    && !p.processing
                    && p.result.is_none(),
                "Invalid or expired sign-in state"
            );
            p.processing = true;
            p.clone()
        };
        let result = if cancelled {
            Err(anyhow::anyhow!(
                "Google sign-in was cancelled. Please try again."
            ))
        } else if let Some(code) = code {
            match &self.google {
                Some(google) => match google.exchange(code, &p.nonce, &p.verifier).await {
                    Ok(identity) => self.enroll(pool, &identity).await,
                    Err(error) => Err(error),
                },
                None => Err(anyhow::anyhow!("Google sign-in has not been configured")),
            }
        } else {
            Err(anyhow::anyhow!("Google did not return a sign-in code"))
        };
        // Error details never include Google's tokens or the authorization code.
        let outcome = result.map_err(|e| {
            if cancelled {
                e.to_string()
            } else {
                "Sign-in could not be completed. Check your account has access, then try again."
                    .into()
            }
        });
        if let Some(p) = self.pending.lock().unwrap().get_mut(id) {
            p.result = Some(outcome);
        }
        Ok(())
    }
    fn admit(&self, identity: &Identity) -> Result<String> {
        ensure!(
            identity.email_verified && !identity.sub.is_empty(),
            "Unverified Google identity"
        );
        if let Some(path) = &self.rules {
            let rules = joppa::access::Rules::read(path)?;
            ensure!(
                !rules
                    .resolve(&identity.email, identity.hd.as_deref())
                    .is_empty(),
                "This account has no workspace access."
            );
            return Ok(self
                .users
                .get(&identity.email.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| format!("user-{:x}", Sha256::digest(identity.sub.as_bytes()))));
        }
        self.users
            .get(&identity.email.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("This account has no access to Joppa."))
    }
    async fn enroll(&self, pool: &deadpool_postgres::Pool, identity: &Identity) -> Result<String> {
        let actor = self.admit(identity)?;
        if let Some(path) = &self.rules {
            let workspaces =
                joppa::access::Rules::read(path)?.resolve(&identity.email, identity.hd.as_deref());
            joppa::store::enroll(
                pool,
                &actor,
                &identity.sub,
                &identity.email.to_ascii_lowercase(),
                identity.hd.as_deref(),
                &workspaces,
            )
            .await?;
        }
        Ok(actor)
    }
    pub async fn poll(&self, id: &str) -> Result<Option<(String, String)>> {
        let mut pending = self.pending.lock().unwrap();
        let p = pending
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Sign-in expired. Please try again."))?;
        if p.expires <= Instant::now() {
            pending.remove(id);
            anyhow::bail!("Sign-in expired. Please try again.");
        }
        let Some(result) = p.result.clone() else {
            return Ok(None);
        };
        pending.remove(id);
        let actor = result.map_err(anyhow::Error::msg)?;
        let token = random();
        Ok(Some((token, actor)))
    }
}
pub fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| {
            let (key, value) = p.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}
pub fn set_cookie(headers: &HeaderMap, name: &str, value: &str, age: u64) -> String {
    let secure = headers
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|o| o.starts_with("https://"));
    format!(
        "{name}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={age}{}",
        if secure { "; Secure" } else { "" }
    )
}
pub fn check_origin(headers: &HeaderMap) -> Result<()> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("origin required"))?;
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("host required"))?;
    ensure!(
        origin == format!("http://{host}") || origin == format!("https://{host}"),
        "cross-origin request denied"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn login() -> Login {
        Login {
            users: BTreeMap::from([("owner@example.test".into(), "owner".into())]),
            rules: None,
            google: Some(
                Google::new(crate::google_auth::Config {
                    client_id: "joppa-client".into(),
                    client_secret: "private".into(),
                    redirect_uri: "https://joppa.example/auth/google/callback".into(),
                })
                .unwrap(),
            ),
            pending: Default::default(),
        }
    }
    #[tokio::test]
    async fn state_is_browser_bound_and_single_use() {
        let l = login();
        let pool = joppa::store::pool(joppa::store::DEFAULT_URL).unwrap();
        let (id, url) = l.start(None).await.unwrap();
        assert!(
            url["verification_url"]
                .as_str()
                .unwrap()
                .starts_with("https://accounts.google.com/")
        );
        assert!(!url.to_string().contains(&id));
        assert!(l.poll(&id).await.unwrap().is_none());
        assert!(l.poll("another-browser").await.is_err());
        assert!(
            l.callback(&pool, &id, "wrong-state", None, true)
                .await
                .is_err()
        );
        let state = l.pending.lock().unwrap()[&id].state.clone();
        assert!(
            l.callback(&pool, "another-browser", &state, None, true)
                .await
                .is_err()
        );
        l.callback(&pool, &id, &state, None, true).await.unwrap();
        assert!(l.callback(&pool, &id, &state, None, true).await.is_err());
        assert!(l.poll(&id).await.is_err());
        let (id, _) = l.start(None).await.unwrap();
        l.pending.lock().unwrap().get_mut(&id).unwrap().result = Some(Ok("owner".into()));
        let (token, actor) = l.poll(&id).await.unwrap().unwrap();
        assert_eq!(actor, "owner");
        assert!(l.poll(&id).await.is_err());
        assert_eq!(token.len(), 64);
        let (id, _) = l.start(None).await.unwrap();
        l.pending.lock().unwrap().get_mut(&id).unwrap().expires =
            Instant::now() - Duration::from_secs(1);
        assert!(l.poll(&id).await.is_err());
    }
    #[test]
    fn domain_enrollment_uses_google_subject_not_colliding_email_initials() {
        let path = std::env::temp_dir().join(format!("joppa-access-{}.json", random()));
        std::fs::write(
            &path,
            r#"{"domains":{"alpha.example":["alpha"],"beta.example":["beta"]}}"#,
        )
        .unwrap();
        let mut l = login();
        l.rules = Some(path.clone());
        let identity = |sub: &str, email: &str, hd: Option<&str>| {
            serde_json::from_value::<Identity>(
                json!({"sub":sub,"email":email,"email_verified":true,"hd":hd,"nonce":"n"}),
            )
            .unwrap()
        };
        let a = l
            .admit(&identity(
                "google-alpha",
                "same@alpha.example",
                Some("alpha.example"),
            ))
            .unwrap();
        let b = l
            .admit(&identity(
                "google-beta",
                "same@beta.example",
                Some("beta.example"),
            ))
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(
            a,
            l.admit(&identity(
                "google-alpha",
                "same@alpha.example",
                Some("alpha.example")
            ))
            .unwrap()
        );
        assert!(
            l.admit(&identity("personal", "same@alpha.example", None))
                .is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn verified_google_email_requires_an_explicit_joppa_invitation() {
        let l = login();
        let identity = |email: &str, verified: bool| {
            serde_json::from_value::<Identity>(
                json!({"sub":"google-1","email":email,"email_verified":verified,"nonce":"n"}),
            )
            .unwrap()
        };
        assert_eq!(
            l.admit(&identity("OWNER@example.test", true)).unwrap(),
            "owner"
        );
        assert!(l.admit(&identity("outsider@example.test", true)).is_err());
        assert!(l.admit(&identity("owner@example.test", false)).is_err());
    }
}
