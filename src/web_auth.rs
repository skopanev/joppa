//! Joppa-owned Google sign-in attempts and browser sessions.
use crate::google_auth::{Google, Identity};
use anyhow::{Result, ensure};
use axum::http::{HeaderMap, header};
use serde_json::{Value, json};
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
struct Session {
    actor: String,
    expires: Instant,
}
#[derive(Clone)]
pub struct Login {
    pub users: BTreeMap<String, String>,
    google: Option<Google>,
    pending: Arc<Mutex<BTreeMap<String, Pending>>>,
    sessions: Arc<Mutex<BTreeMap<String, Session>>>,
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
        }
        Ok(Self {
            users: users
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            google: Google::from_env()?,
            pending: Default::default(),
            sessions: Default::default(),
        })
    }
    pub async fn start(&self, previous: Option<&str>) -> Result<(String, Value)> {
        let google = self.google.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Google sign-in is being configured for Joppa. Please try again later.")
        })?;
        ensure!(
            !self.users.is_empty(),
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
                Some(google) => google
                    .exchange(code, &p.nonce, &p.verifier)
                    .await
                    .and_then(|identity| self.admit(&identity)),
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
        self.users
            .get(&identity.email.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("This account has no access to Joppa."))
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
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| s.expires > Instant::now());
        sessions.insert(
            token.clone(),
            Session {
                actor: actor.clone(),
                expires: Instant::now() + Duration::from_secs(43200),
            },
        );
        Ok(Some((token, actor)))
    }
    pub fn actor(&self, token: &str) -> Option<String> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| s.expires > Instant::now());
        sessions.get(token).map(|s| s.actor.clone())
    }
    pub fn logout(&self, token: &str) {
        self.sessions.lock().unwrap().remove(token);
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
            google: Some(
                Google::new(crate::google_auth::Config {
                    client_id: "joppa-client".into(),
                    client_secret: "private".into(),
                    redirect_uri: "https://joppa.example/auth/google/callback".into(),
                })
                .unwrap(),
            ),
            pending: Default::default(),
            sessions: Default::default(),
        }
    }
    #[tokio::test]
    async fn state_is_browser_bound_single_use_and_sessions_are_revocable() {
        let l = login();
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
        assert!(l.callback(&id, "wrong-state", None, true).await.is_err());
        let state = l.pending.lock().unwrap()[&id].state.clone();
        assert!(
            l.callback("another-browser", &state, None, true)
                .await
                .is_err()
        );
        l.callback(&id, &state, None, true).await.unwrap();
        assert!(l.callback(&id, &state, None, true).await.is_err());
        assert!(l.poll(&id).await.is_err());
        let (id, _) = l.start(None).await.unwrap();
        l.pending.lock().unwrap().get_mut(&id).unwrap().result = Some(Ok("owner".into()));
        let (token, actor) = l.poll(&id).await.unwrap().unwrap();
        assert_eq!(actor, "owner");
        assert!(l.poll(&id).await.is_err());
        assert_eq!(l.actor(&token), Some("owner".into()));
        l.logout(&token);
        assert!(l.actor(&token).is_none());
        let (id, _) = l.start(None).await.unwrap();
        l.pending.lock().unwrap().get_mut(&id).unwrap().expires =
            Instant::now() - Duration::from_secs(1);
        assert!(l.poll(&id).await.is_err());
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
