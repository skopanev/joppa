//! Black-box HTTP, MCP and Postgres tests for P1 items 6 to 10 and P2 item 13:
//! ac:fgldeoxa, ac:9oh0z9aq, ac:bvkl297c, ac:i6ht2djy and ac:ytqs5gst.
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use joppa::{commands::Command as Operation, sessions, store};
use read_key::{Auth, call, session};
use record_world::{World, fake_spaces};
use save_world::{head, ok, refused, replay, save};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

async fn mcp_bare(
    w: &World,
    token: &str,
    tool: &str,
    args: Value,
) -> Result<Result<Value, String>> {
    let (code, reply) = w.mcp_raw(Some(token), tool, args).await?;
    ensure!(code == 200, "{tool}: {code} {reply}");
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .with_context(|| format!("MCP reply: {reply}"))?;
    Ok(if reply["result"]["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text)?)
    })
}

async fn mcp_ok(w: &World, token: &str, tool: &str, args: Value) -> Result<Value> {
    w.mcp(token, tool, args)
        .await?
        .map_err(|e| anyhow::anyhow!("{tool}: {e}"))
}

async fn http_get(w: &World, query: &str) -> Result<Value> {
    let path = format!("/api/workspaces/{}/objects?{query}", w.ws);
    let (code, body) = call(
        &w.http,
        &w.root,
        "GET",
        &path,
        &[Auth::Bearer(&w.bot)],
        None,
    )
    .await?;
    ensure!(code == 200, "GET {path}: {code} {body}");
    Ok(body)
}

async fn oauth_token_for(w: &World, actor: &str) -> Result<String> {
    let raw_cookie = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&w.pool, &raw_cookie, actor, None).await?;
    let cookie = format!("joppa_session={raw_cookie}");
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let redirect = "http://127.0.0.1:32123/callback";
    let reg: Value = http
        .post(format!("{}/oauth/register", w.root))
        .json(&json!({"client_name":"P1 check","redirect_uris":[redirect],"token_endpoint_auth_method":"none"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let client_id = reg["client_id"].as_str().context("client_id")?;
    let verifier = "p1-pkce-verifier-abcdefghijklmnopqrstuvwxyz0123456789012345";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let resource = format!("{}/mcp", w.root);
    let approved: Value = http
        .post(format!("{}/oauth/approve", w.root))
        .header("Origin", &w.root)
        .header("Cookie", &cookie)
        .json(&json!({
            "client_id": client_id, "redirect_uri": redirect, "response_type": "code",
            "code_challenge": challenge, "code_challenge_method": "S256",
            "state": "st", "scope": "joppa", "resource": resource
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let loc = approved["redirect"].as_str().context("redirect")?;
    let code = reqwest::Url::parse(loc)?
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
    exchanged["access_token"]
        .as_str()
        .map(str::to_owned)
        .context("access_token")
}

async fn create_write_key(w: &World) -> Result<String> {
    let cookie = session(w, "owner").await?;
    let auth = [Auth::Cookie(&cookie), Auth::Origin(&w.root)];
    let path = format!("/api/workspaces/{}/write-key", w.ws);
    let (code, body) = call(&w.http, &w.root, "POST", &path, &auth, None).await?;
    ensure!(code == 200, "POST write-key: {code} {body}");
    body["key"].as_str().map(str::to_owned).context("write key")
}

include!("p1_feedback_http/part01.rs");
include!("p1_feedback_http/part02.rs");
include!("p1_feedback_http/part03.rs");
