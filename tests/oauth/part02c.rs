// Part of tests/oauth/part02.rs: the last steps of the test, included after part02b.

/// A fresh consent after revocation, logout, hosted and loopback callbacks and manual codes.
#[allow(clippy::too_many_arguments)]
async fn fresh_consent_logout_and_manual_codes(
    pool: &deadpool_postgres::Pool,
    dir: &std::path::Path,
    base: &str,
    resource: &str,
    http: &reqwest::Client,
    probe: &Value,
    id: &str,
    verifier: &str,
    a: &Value,
    cookie: &str,
    grant: &Value,
) -> Result<()> {
    // A fresh consent can recover from revocation; explicit logout also revokes its family.
    let accepted: Value = http
        .post(format!("{base}/oauth/approve"))
        .header("Origin", base)
        .header("Cookie", cookie)
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
    let fresh: Value = exchange(http, base, &fresh).await?.json().await?;
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
        http.post(resource)
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
            .header("Origin", base)
            .header("Cookie", cookie)
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
        assert_eq!(exchange(http, base, &grant).await?.status(), 200);
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
        .header("Origin", base)
        .header("Cookie", cookie)
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
    let r = exchange(http, base, &grant).await?;
    assert_eq!(r.status(), 200);
    let token: Value = r.json().await?;
    assert_eq!(
        http.post(resource)
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
