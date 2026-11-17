//! Black-box checks of Read-only (viewer) MCP and HTTP access (requirement:m78flwn8 and
//! ac:twi9vco1).
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use joppa::{commands::Command as Operation, sessions, store};
use record_world::{World, fake_spaces};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Issue a real browser session cookie for `actor`.
async fn browser_session(pool: &deadpool_postgres::Pool, actor: &str) -> Result<String> {
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(pool, &token, actor, None).await?;
    Ok(format!("joppa_session={token}"))
}

/// Issue a real MCP OAuth bearer token (jpa_...) for `actor` through register, approve and token.
async fn oauth_token(w: &World, cookie: &str) -> Result<String> {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let redirect = "http://127.0.0.1:32123/callback";
    let reg: Value = http
        .post(format!("{}/oauth/register", w.root))
        .json(&json!({"client_name":"Readonly check","redirect_uris":[redirect],"token_endpoint_auth_method":"none"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let client_id = reg["client_id"].as_str().context("client_id")?;
    let verifier = "readonly-pkce-verifier-abcdefghijklmnopqrstuvwxyz0123456789";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let resource = format!("{}/mcp", w.root);
    let approved: Value = http
        .post(format!("{}/oauth/approve", w.root))
        .header("Origin", &w.root)
        .header("Cookie", cookie)
        .json(&json!({
            "client_id": client_id,
            "redirect_uri": redirect,
            "response_type": "code",
            "code_challenge": challenge,
            "code_challenge_method": "S256",
            "state": "st",
            "scope": "joppa",
            "resource": resource
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let loc = approved["redirect"].as_str().context("redirect")?;
    let parsed = reqwest::Url::parse(loc)?;
    let code = parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .context("oauth code")?;
    let mut form = reqwest::Url::parse("http://form.example")?;
    form.query_pairs_mut()
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &code)
        .append_pair("redirect_uri", redirect)
        .append_pair("client_id", client_id)
        .append_pair("code_verifier", verifier)
        .append_pair("resource", &resource);
    let exchanged: Value = http
        .post(format!("{}/oauth/token", w.root))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form.query().unwrap_or_default().to_owned())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(exchanged["access_token"]
        .as_str()
        .context("access_token")?
        .to_owned())
}

/// Enroll a bound Google viewer `maria` on workspace `ws` and return `(actor, cookie, email)`.
async fn enroll_viewer_maria(
    pool: &deadpool_postgres::Pool,
    ws: &str,
) -> Result<(String, String, String)> {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let actor = format!("google-maria-{suffix}");
    let email = format!("{actor}@example.com");
    pool.get()
        .await?
        .execute(
            "INSERT INTO joppa.google_identities(actor,google_sub,email,display_name) VALUES($1,$1,$2,'Maria')",
            &[&actor, &email],
        )
        .await?;
    store::grant_email(pool, ws, "owner", &email).await?;
    store::enroll_granted(pool, &actor, None, None).await?;
    let cookie = browser_session(pool, &actor).await?;
    Ok((actor, cookie, email))
}

/// Send one raw JSON-RPC request to `/mcp`.
async fn mcp_rpc(w: &World, token: &str, method: &str, params: Value) -> Result<Value> {
    Ok(w.http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

include!("readonly_mcp_http/part01.rs");
include!("readonly_mcp_http/part02.rs");
