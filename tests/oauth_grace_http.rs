//! Black-box check of the 60 s OAuth refresh token grace window (ac:7rslc81s).
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use joppa::sessions;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

fn form(pairs: &[(&str, &str)]) -> String {
    let mut u = reqwest::Url::parse("http://form.example").unwrap();
    for (k, v) in pairs {
        u.query_pairs_mut().append_pair(k, v);
    }
    u.query().unwrap_or_default().to_owned()
}

async fn post_token(http: &reqwest::Client, root: &str, body: String) -> Result<(u16, Value)> {
    let res = http
        .post(format!("{root}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    Ok((res.status().as_u16(), res.json().await?))
}

async fn issue_code(
    http: &reqwest::Client,
    root: &str,
    cookie: &str,
    client_id: &str,
    redirect: &str,
    verifier: &str,
) -> Result<String> {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let resource = format!("{root}/mcp");
    let approved: Value = http
        .post(format!("{root}/oauth/approve"))
        .header("Origin", root)
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
    let parsed = reqwest::Url::parse(approved["redirect"].as_str().context("redirect")?)?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .context("code")
}

// ac:7rslc81s
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn refresh_token_resent_within_grace_keeps_its_family_and_late_replay_revokes_it()
-> Result<()> {
    let w = record_world::world_env(
        &["tests/x.rs > t"],
        &[("JOPPA_REFRESH_GRACE_SECONDS", "2".into())],
    )
    .await?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let redirect = "http://127.0.0.1:32123/callback";
    let resource = format!("{}/mcp", w.root);
    let verifier = "grace-pkce-verifier-abcdefghijklmnopqrstuvwxyz0123456789";
    let sess = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&w.pool, &sess, "owner", None).await?;
    let cookie = format!("joppa_session={sess}");

    let reg: Value = http
        .post(format!("{}/oauth/register", w.root))
        .json(&json!({"client_name":"Grace C1","redirect_uris":[redirect],"token_endpoint_auth_method":"none"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let c1 = reg["client_id"].as_str().context("c1")?.to_owned();
    let code1 = issue_code(&http, &w.root, &cookie, &c1, redirect, verifier).await?;
    let (st1, pair1) = post_token(
        &http,
        &w.root,
        form(&[
            ("grant_type", "authorization_code"),
            ("client_id", &c1),
            ("code", &code1),
            ("redirect_uri", redirect),
            ("code_verifier", verifier),
            ("resource", &resource),
        ]),
    )
    .await?;
    ensure!(st1 == 200, "{pair1}");
    let r1 = pair1["refresh_token"].as_str().context("r1")?.to_owned();

    // Exchange R1 -> A2, R2. Within G=2s send R1 again -> A3, R3.
    let exchange_r = |tok: &str| {
        post_token(
            &http,
            &w.root,
            form(&[
                ("grant_type", "refresh_token"),
                ("client_id", &c1),
                ("refresh_token", tok),
                ("resource", &resource),
            ]),
        )
    };
    let (st2, pair2) = exchange_r(&r1).await?;
    let (st3, pair3) = exchange_r(&r1).await?;
    ensure!(st2 == 200 && st3 == 200, "{pair2} / {pair3}");
    let (a2, r2) = (
        pair2["access_token"].as_str().context("a2")?.to_owned(),
        pair2["refresh_token"].as_str().context("r2")?.to_owned(),
    );
    let (a3, r3) = (
        pair3["access_token"].as_str().context("a3")?.to_owned(),
        pair3["refresh_token"].as_str().context("r3")?.to_owned(),
    );
    ensure!(a2 != a3 && r2 != r3);

    // Both A2 and A3 work on /mcp, and R2 rotates cleanly to A4, R4.
    let ping = json!({"jsonrpc":"2.0","id":1,"method":"ping","params":{}});
    for tok in [&a2, &a3] {
        let code = http
            .post(&resource)
            .bearer_auth(tok)
            .json(&ping)
            .send()
            .await?
            .status()
            .as_u16();
        ensure!(code == 200, "access failed: {code}");
    }
    let (st4, pair4) = exchange_r(&r2).await?;
    ensure!(st4 == 200, "{pair4}");
    let (a4, r4) = (
        pair4["access_token"].as_str().context("a4")?.to_owned(),
        pair4["refresh_token"].as_str().context("r4")?.to_owned(),
    );

    // A second client sends one authorization code twice within G: second gets 400 invalid_grant.
    let reg2: Value = http
        .post(format!("{}/oauth/register", w.root))
        .json(&json!({"client_name":"Grace C2","redirect_uris":[redirect],"token_endpoint_auth_method":"none"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let c2 = reg2["client_id"].as_str().context("c2")?.to_owned();
    let code2 = issue_code(&http, &w.root, &cookie, &c2, redirect, verifier).await?;
    let code_form = || {
        form(&[
            ("grant_type", "authorization_code"),
            ("client_id", &c2),
            ("code", &code2),
            ("redirect_uri", redirect),
            ("code_verifier", verifier),
            ("resource", &resource),
        ])
    };
    let (ok_c2, _) = post_token(&http, &w.root, code_form()).await?;
    let (bad_c2, err_c2) = post_token(&http, &w.root, code_form()).await?;
    ensure!(
        ok_c2 == 200 && bad_c2 == 400 && err_c2["error"] == "invalid_grant",
        "{bad_c2} {err_c2}"
    );

    // After G=2s has passed since the first exchange of R1, sending R1 again revokes all of F.
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let (late_st, late_body) = exchange_r(&r1).await?;
    ensure!(
        late_st == 400 && late_body["error"] == "invalid_grant",
        "{late_st} {late_body}"
    );
    for tok in [&a2, &a3, &a4] {
        let code = http
            .post(&resource)
            .bearer_auth(tok)
            .json(&ping)
            .send()
            .await?
            .status()
            .as_u16();
        ensure!(code == 401, "expected 401 after family revoke, got {code}");
    }
    for rtok in [&r3, &r4] {
        let (st, body) = exchange_r(rtok).await?;
        ensure!(
            st == 400 && body["error"] == "invalid_grant",
            "expected invalid_grant for {rtok}, got {st} {body}"
        );
    }
    Ok(())
}
