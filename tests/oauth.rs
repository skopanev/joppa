use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use joppa::{commands::Command as Operation, sessions, store};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn start(url: &str, tokens: &std::path::Path, port: u16) -> Result<Server> {
    let base = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", url)
            .env("JOPPA_TOKENS_FILE", tokens)
            .env("JOPPA_PUBLIC_URL", &base)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("{base}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(server);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}
fn form(v: &Value) -> String {
    let mut url = reqwest::Url::parse("http://form.example").unwrap();
    let mut s = url.query_pairs_mut();
    for (k, v) in v.as_object().unwrap() {
        s.append_pair(k, v.as_str().unwrap());
    }
    drop(s);
    url.query().unwrap().to_owned()
}
async fn exchange(http: &reqwest::Client, base: &str, v: &Value) -> Result<reqwest::Response> {
    Ok(http
        .post(format!("{base}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form(v))
        .send()
        .await?)
}
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn remote_oauth_pkce_isolation_rotation_restart_and_revocation() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(db.starts_with("joppa_test"), "isolated database required");
    store::migrate(&pool).await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let owner = format!("oauth-owner-{suffix}");
    let ws = format!("oauth-{suffix}");
    let other = format!("other-{suffix}");
    let make: Operation = serde_json::from_value(json!({"op":"workspace","title":"OAuth test"}))?;
    store::execute(&pool, &ws, &owner, "create", &make).await?;
    store::execute(&pool, &other, "other-owner", "create", &make).await?;
    let dir = std::env::temp_dir().join(format!("joppa-oauth-{suffix}"));
    std::fs::create_dir_all(&dir)?;
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({"existing-private-service-credential":owner}).to_string(),
    )?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let base = format!("http://127.0.0.1:{port}");
    let resource = format!("{base}/mcp");
    let server = start(&url, &tokens, port).await?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let probe = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}});
    for method in [reqwest::Method::POST, reqwest::Method::GET] {
        let r = http.request(method, &resource).json(&probe).send().await?;
        assert_eq!(r.status(), 401);
        assert!(
            r.headers()["www-authenticate"]
                .to_str()?
                .contains("/.well-known/oauth-protected-resource/mcp")
        );
        assert!(
            r.headers()["www-authenticate"]
                .to_str()?
                .contains("codex mcp login joppa")
        );
        let body: Value = r.json().await?;
        assert!(
            body["error_description"]
                .as_str()
                .unwrap()
                .contains("codex mcp login joppa")
        );
    }
    let meta: Value = http
        .get(format!("{base}/.well-known/oauth-protected-resource/mcp"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(meta["resource"], resource);
    let meta: Value = http
        .get(format!("{base}/.well-known/oauth-authorization-server"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(meta["code_challenge_methods_supported"], json!(["S256"]));
    let invalid = http
        .post(format!("{base}/oauth/register"))
        .json(&json!({"redirect_uris":["http://localhost.attacker.example/callback"]}))
        .send()
        .await?;
    assert_eq!(invalid.status(), 400);
    let redirect = "http://127.0.0.1:32123/callback";
    let client:Value=http.post(format!("{base}/oauth/register")).json(&json!({"client_name":"Integration client","redirect_uris":[redirect],"token_endpoint_auth_method":"none"})).send().await?.json().await?;
    let id = client["client_id"].as_str().unwrap();
    let verifier = "test-pkce-verifier-abcdefghijklmnopqrstuvwxyz0123456789";
    let a = json!({"client_id":id,"redirect_uri":redirect,"response_type":"code","code_challenge":URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),"code_challenge_method":"S256","state":"opaque+client&state=/?","scope":"joppa","resource":resource});
    let browser = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &browser, &owner).await?;
    let cookie = format!("joppa_session={browser}");
    assert_eq!(
        http.get(format!("{base}/oauth/authorize?{}", form(&a)))
            .send()
            .await?
            .status(),
        200
    );
    let mut bad = a.clone();
    bad["redirect_uri"] = json!("https://attacker.example/callback");
    assert_eq!(
        http.get(format!("{base}/oauth/authorize?{}", form(&bad)))
            .send()
            .await?
            .status(),
        400
    );
    bad = a.clone();
    bad["resource"] = json!("https://attacker.example/mcp");
    assert_eq!(
        http.get(format!("{base}/oauth/authorize?{}", form(&bad)))
            .send()
            .await?
            .status(),
        400
    );
    for (origin, auth) in [(base.as_str(), false), ("https://attacker.example", true)] {
        let mut request = http
            .post(format!("{base}/oauth/approve"))
            .header("Origin", origin)
            .json(&a);
        if auth {
            request = request.header("Cookie", &cookie);
        }
        assert!(!request.send().await?.status().is_success());
    }
    assert_eq!(
        http.post(format!("{base}/oauth/approve"))
            .header("Origin", &base)
            .bearer_auth("existing-private-service-credential")
            .json(&a)
            .send()
            .await?
            .status(),
        400
    );
    let accepted: Value = http
        .post(format!("{base}/oauth/approve"))
        .header("Origin", &base)
        .header("Cookie", &cookie)
        .json(&a)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(accepted["copy_link"], true);
    assert_eq!(accepted["expires_in"], 300);
    let callback = reqwest::Url::parse(accepted["redirect"].as_str().unwrap())?;
    let pairs: std::collections::BTreeMap<_, _> = callback.query_pairs().into_owned().collect();
    assert_eq!(pairs["state"], "opaque+client&state=/?");
    let grant = json!({"grant_type":"authorization_code","client_id":id,"code":pairs["code"],"redirect_uri":redirect,"code_verifier":verifier,"resource":resource});
    for (field, value) in [
        (
            "code_verifier",
            "wrong-verifier-abcdefghijklmnopqrstuvwxyz0123456789",
        ),
        ("redirect_uri", "http://127.0.0.1:32123/other"),
        ("client_id", "other-client"),
        ("resource", "https://attacker.example/mcp"),
    ] {
        let mut bad = grant.clone();
        bad[field] = json!(value);
        assert_eq!(exchange(&http, &base, &bad).await?.status(), 400);
    }
    let r = exchange(&http, &base, &grant).await?;
    assert_eq!(r.status(), 200);
    let issued: Value = r.json().await?;
    let access = issued["access_token"].as_str().unwrap();
    let refresh = issued["refresh_token"].as_str().unwrap();
    assert_eq!(exchange(&http, &base, &grant).await?.status(), 400);
    assert_eq!(
        http.post(&resource)
            .header("Cookie", &cookie)
            .json(&probe)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.post(&resource)
            .bearer_auth(refresh)
            .json(&probe)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.get(format!("{base}/api/me"))
            .bearer_auth(access)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.post(&resource)
            .bearer_auth(access)
            .header("Origin", "https://attacker.example")
            .json(&probe)
            .send()
            .await?
            .status(),
        403
    );
    drop(server);
    let _server = start(&url, &tokens, port).await?;
    let r: Value = http
        .post(&resource)
        .bearer_auth(access)
        .json(&probe)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert!(
        r["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("codex mcp login joppa")
    );
    assert_eq!(
        http.post(&resource)
            .bearer_auth(access)
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await?
            .status(),
        202
    );
    for (target, denied) in [(&ws, false), (&other, true)] {
        let r:Value=http.post(&resource).bearer_auth(access).json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"joppa_read","arguments":{"workspace":target}}})).send().await?.json().await?;
        assert_eq!(r["result"]["isError"] == true, denied);
    }
    let rotation = json!({"grant_type":"refresh_token","client_id":id,"refresh_token":refresh,"resource":resource});
    let next: Value = exchange(&http, &base, &rotation).await?.json().await?;
    let next_access = next["access_token"].as_str().unwrap();
    assert_ne!(next_access, access);
    assert_eq!(
        http.post(&resource)
            .bearer_auth(next_access)
            .json(&probe)
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(exchange(&http, &base, &rotation).await?.status(), 400);
    for key in [access, next_access] {
        assert_eq!(
            http.post(&resource)
                .bearer_auth(key)
                .json(&probe)
                .send()
                .await?
                .status(),
            401
        );
    }
    let r=exchange(&http,&base,&json!({"grant_type":"refresh_token","client_id":id,"refresh_token":next["refresh_token"],"resource":resource})).await?;
    assert_eq!(r.status(), 400);
    // A fresh consent can recover from revocation; explicit logout also revokes its family.
    let accepted: Value = http
        .post(format!("{base}/oauth/approve"))
        .header("Origin", &base)
        .header("Cookie", &cookie)
        .json(&a)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(accepted["copy_link"], true);
    assert_eq!(accepted["expires_in"], 300);
    let callback = reqwest::Url::parse(accepted["redirect"].as_str().unwrap())?;
    let code = callback
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned();
    let mut fresh = grant.clone();
    fresh["code"] = json!(code);
    let fresh: Value = exchange(&http, &base, &fresh).await?.json().await?;
    let token = fresh["access_token"].as_str().unwrap();
    assert_eq!(
        http.post(format!("{base}/oauth/revoke"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form(&json!({"token":token,"client_id":id})))
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        http.post(&resource)
            .bearer_auth(token)
            .json(&probe)
            .send()
            .await?
            .status(),
        401
    );
    assert!(
        pool.get()
            .await?
            .execute("DELETE FROM joppa.oauth_tokens WHERE client_id=$1", &[&id])
            .await
            .is_err()
    );
    // Hosted callbacks keep redirecting; exact loopback hosts offer a copyable link.
    for (redirect, copy_link) in [
        ("https://client.example/callback?existing=yes", false),
        ("https://localhost.example/callback", false),
        ("http://localhost:45678/callback", true),
        ("http://[::1]:45678/callback", true),
    ] {
        let client: Value = http
            .post(format!("{base}/oauth/register"))
            .json(&json!({"redirect_uris":[redirect]}))
            .send()
            .await?
            .json()
            .await?;
        let mut request = a.clone();
        request["client_id"] = client["client_id"].clone();
        request["redirect_uri"] = json!(redirect);
        let details: Value = http
            .get(format!("{base}/oauth/details?{}", form(&request)))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(details["copy_link"], copy_link);
        let approved: Value = http
            .post(format!("{base}/oauth/approve"))
            .header("Origin", &base)
            .header("Cookie", &cookie)
            .json(&request)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(approved["copy_link"], copy_link);
        assert!(approved.get("code").is_none());
        let callback = reqwest::Url::parse(approved["redirect"].as_str().unwrap())?;
        let query: std::collections::BTreeMap<_, _> = callback.query_pairs().into_owned().collect();
        assert_eq!(query["state"], a["state"].as_str().unwrap());
        if redirect.contains("existing=yes") {
            assert_eq!(query["existing"], "yes");
        }
        let grant = json!({"grant_type":"authorization_code","client_id":client["client_id"],
            "code":query["code"],"redirect_uri":redirect,"code_verifier":verifier,"resource":resource});
        assert_eq!(exchange(&http, &base, &grant).await?.status(), 200);
    }
    // Antigravity's manual code entry does not depend on its hosted callback page.
    let manual_redirect = "https://antigravity.google/oauth-callback";
    let client: Value = http
        .post(format!("{base}/oauth/register"))
        .json(&json!({"client_name":"Antigravity","redirect_uris":[manual_redirect]}))
        .send()
        .await?
        .json()
        .await?;
    let mut manual = a.clone();
    manual["client_id"] = client["client_id"].clone();
    manual["redirect_uri"] = json!(manual_redirect);
    let details: Value = http
        .get(format!("{base}/oauth/details?{}", form(&manual)))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(details["manual"], true);
    let issued: Value = http
        .post(format!("{base}/oauth/approve"))
        .header("Origin", &base)
        .header("Cookie", &cookie)
        .json(&manual)
        .send()
        .await?
        .json()
        .await?;
    assert!(issued.get("redirect").is_none());
    assert_eq!(issued["expires_in"], 300);
    let mut grant = grant.clone();
    grant["client_id"] = client["client_id"].clone();
    grant["redirect_uri"] = json!(manual_redirect);
    grant["code"] = issued["code"].clone();
    let r = exchange(&http, &base, &grant).await?;
    assert_eq!(r.status(), 200);
    let token: Value = r.json().await?;
    assert_eq!(
        http.post(&resource)
            .bearer_auth(token["access_token"].as_str().unwrap())
            .json(&probe)
            .send()
            .await?
            .status(),
        200
    );
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
