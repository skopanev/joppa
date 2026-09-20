//! Google sign-in through NTK's existing device flow. Credentials stay on the server.
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
    code: String,
    secret: String,
    expires: Instant,
}
#[derive(Clone)]
struct Session {
    actor: String,
    expires: Instant,
}
#[derive(Clone)]
pub struct Login {
    pub users: BTreeMap<String, String>,
    base: String,
    client: reqwest::Client,
    pending: Arc<Mutex<BTreeMap<String, Pending>>>,
    sessions: Arc<Mutex<BTreeMap<String, Session>>>,
}
impl Login {
    pub fn from_env() -> Result<Self> {
        let users: BTreeMap<String, String> = match std::env::var("JOPPA_LOGIN_USERS_FILE") {
            Ok(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
            Err(_) => BTreeMap::new(),
        };
        for actor in users.values() {
            joppa::commands::identifier(actor)?;
        }
        let base = std::env::var("JOPPA_NTK_URL")
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_owned();
        if !base.is_empty() {
            let url = reqwest::Url::parse(&base)?;
            ensure!(
                url.scheme() == "https"
                    || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1")),
                "login service must use HTTPS"
            );
        }
        Ok(Self {
            users,
            base,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            pending: Default::default(),
            sessions: Default::default(),
        })
    }
    pub async fn start(&self, previous: Option<&str>) -> Result<(String, Value)> {
        ensure!(
            !self.base.is_empty() && !self.users.is_empty(),
            "Google sign-in has not been configured"
        );
        {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|_, p| p.expires > Instant::now());
            if let Some(id) = previous {
                pending.remove(id);
            }
            ensure!(
                pending.len() < 100,
                "Too many sign-in attempts. Please try again later."
            );
        }
        let r = self
            .client
            .post(format!("{}/device/start", self.base))
            .send()
            .await?
            .error_for_status()?;
        let data: Value = r.json().await?;
        let code = text(&data, "code")?.to_owned();
        let secret = text(&data, "device_secret")?.to_owned();
        // Construct the URL ourselves; never forward a provider-supplied arbitrary redirect.
        let mut url = reqwest::Url::parse(&format!("{}/link", self.base))?;
        url.query_pairs_mut().append_pair("code", &code);
        let ttl = data["expires_in"].as_u64().unwrap_or(300).min(600);
        let id = uuid::Uuid::new_v4().to_string();
        self.pending.lock().unwrap().insert(
            id.clone(),
            Pending {
                code,
                secret,
                expires: Instant::now() + Duration::from_secs(ttl),
            },
        );
        Ok((
            id,
            json!({"verification_url": url.as_str(), "expires_in": ttl}),
        ))
    }
    pub async fn poll(&self, id: &str) -> Result<Option<(String, String)>> {
        // Remove while in flight: concurrent polls cannot collect a credential twice.
        let pending = self
            .pending
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| anyhow::anyhow!("Sign-in expired. Please try again."))?;
        ensure!(
            pending.expires > Instant::now(),
            "Sign-in expired. Please try again."
        );
        let response = self
            .client
            .post(format!("{}/device/poll", self.base))
            .json(&json!({"code": pending.code, "device_secret": pending.secret}))
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::ACCEPTED {
            self.pending.lock().unwrap().insert(id.to_owned(), pending);
            return Ok(None);
        }
        let data: Value = response.error_for_status()?.json().await?;
        let key = text(&data, "key")?;
        let me: Value = self
            .client
            .get(format!("{}/v1/me", self.base))
            .bearer_auth(key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let actor = self
            .users
            .get(text(&me, "user_id")?)
            .ok_or_else(|| anyhow::anyhow!("This account has no access to Joppa."))?
            .clone();
        let token = uuid::Uuid::new_v4().to_string();
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
fn text<'a>(data: &'a Value, field: &str) -> Result<&'a str> {
    data[field]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid sign-in service response"))
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
