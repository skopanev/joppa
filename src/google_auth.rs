//! Joppa's own Google OAuth client. No other product participates in sign-in.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;

#[derive(Clone, Deserialize)]
pub struct Config {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}
#[derive(Clone)]
pub struct Google {
    pub config: Config,
    client: reqwest::Client,
    token_url: String,
    keys_url: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Identity {
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
    #[serde(default)]
    pub hd: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub given_name: Option<String>,
    #[serde(default)]
    pub family_name: Option<String>,
    nonce: String,
}
impl Identity {
    pub fn display_name(&self) -> Option<String> {
        let name = self.name.as_deref().unwrap_or("").trim();
        let name = if name.is_empty() {
            [self.given_name.as_deref(), self.family_name.as_deref()]
                .into_iter()
                .flatten()
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            name.to_owned()
        };
        (!name.is_empty() && name.chars().count() <= 200 && !name.chars().any(char::is_control))
            .then_some(name)
    }
}
#[derive(Deserialize)]
struct Keys {
    keys: Vec<Key>,
}
#[derive(Deserialize)]
struct Key {
    kid: String,
    n: String,
    e: String,
}
impl Google {
    pub fn from_env() -> Result<Option<Self>> {
        let Ok(path) = std::env::var("JOPPA_GOOGLE_CLIENT_FILE") else {
            return Ok(None);
        };
        let config: Config = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        let uri = reqwest::Url::parse(&config.redirect_uri)?;
        ensure!(
            uri.scheme() == "https"
                || (uri.scheme() == "http"
                    && matches!(uri.host_str(), Some("localhost" | "127.0.0.1"))),
            "Google redirect must use HTTPS (except localhost)"
        );
        ensure!(
            uri.path() == "/auth/google/callback"
                && uri.query().is_none()
                && uri.fragment().is_none(),
            "redirect URI must end in /auth/google/callback"
        );
        ensure!(
            !config.client_id.is_empty() && !config.client_secret.is_empty(),
            "Google client credentials are empty"
        );
        Ok(Some(Self::new(config)?))
    }
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            token_url: "https://oauth2.googleapis.com/token".into(),
            keys_url: "https://www.googleapis.com/oauth2/v3/certs".into(),
        })
    }
    pub fn authorize(&self, state: &str, nonce: &str, verifier: &str) -> String {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.config.client_id.as_str()),
            ("redirect_uri", &self.config.redirect_uri),
            ("response_type", "code"),
            ("scope", "openid email profile"),
            ("prompt", "select_account"),
            ("state", state),
            ("nonce", nonce),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        url.into()
    }
    pub async fn exchange(&self, code: &str, nonce: &str, verifier: &str) -> Result<Identity> {
        #[derive(Deserialize)]
        struct Tokens {
            id_token: String,
        }
        let token: Tokens = self
            .client
            .post(&self.token_url)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", &self.config.client_secret),
                ("redirect_uri", &self.config.redirect_uri),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", verifier),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let keys: Keys = self
            .client
            .get(&self.keys_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        verify(&token.id_token, &self.config.client_id, nonce, &keys)
    }
    /// Exchanges codes at a local test token endpoint.
    #[cfg(test)]
    pub(crate) fn with_token_url(mut self, url: String) -> Self {
        self.token_url = url;
        self
    }
}
fn verify(token: &str, client_id: &str, nonce: &str, keys: &Keys) -> Result<Identity> {
    let header = decode_header(token)?;
    ensure!(
        header.alg == Algorithm::RS256,
        "Invalid identity signature algorithm"
    );
    let kid = header.kid.context("Missing signing key ID")?;
    let key = keys
        .keys
        .iter()
        .find(|key| key.kid == kid)
        .context("Unknown signing key")?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
    validation.leeway = 0;
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let identity = decode::<Identity>(
        token,
        &DecodingKey::from_rsa_components(&key.n, &key.e)?,
        &validation,
    )?
    .claims;
    ensure!(
        identity.email_verified && !identity.sub.is_empty(),
        "Google identity is not verified"
    );
    ensure!(identity.nonce == nonce, "Sign-in nonce did not match");
    Ok(identity)
}
#[cfg(test)]
#[path = "google_auth_tests.rs"]
mod tests;
