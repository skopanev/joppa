//! Part of `mcp_oauth`: `approve` and the items after it, moved from mcp_oauth.rs unchanged.
use super::*;

pub(crate) async fn approve(
    State(app): State<App>,
    headers: HeaderMap,
    Json(a): Json<Authorization>,
) -> OAuthResult {
    web_auth::check_origin(&headers).map_err(invalid)?;
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(error("invalid_request", "Approve in your browser"));
    }
    let actor = authenticate(&app, &headers)
        .await
        .map_err(IntoResponse::into_response)?;
    validate(&app, &a).await?;
    let spaces = store::list(&app.pool, &actor).await.map_err(internal)?;
    if spaces.as_array().is_none_or(|s| s.is_empty()) {
        return Err(error(
            "access_denied",
            "Your account has no workspace access",
        ));
    }
    let code = random();
    let resource = format!("{}/mcp", base(&app).map_err(IntoResponse::into_response)?);
    app.pool.get().await.map_err(internal)?.execute("INSERT INTO joppa.oauth_codes(hash,actor,client_id,redirect_uri,challenge,resource,expires_at) VALUES($1,$2,$3,$4,$5,$6,clock_timestamp()+interval '5 minutes')",&[&hash(&code),&actor,&a.client_id,&a.redirect_uri,&a.code_challenge,&resource]).await.map_err(internal)?;
    if manual_callback(&a.redirect_uri) {
        return Ok(Json(json!({"code":code,"expires_in":300})).into_response());
    }
    let mut url = reqwest::Url::parse(&a.redirect_uri).map_err(invalid)?;
    url.query_pairs_mut().append_pair("code", &code);
    if let Some(state) = a.state {
        url.query_pairs_mut().append_pair("state", &state);
    }
    Ok(Json(json!({"redirect":url.as_str(),"copy_link":copy_link_callback(&a.redirect_uri),"expires_in":300})).into_response())
}

#[derive(Deserialize)]
pub(crate) struct Exchange {
    pub(crate) grant_type: String,
    pub(crate) client_id: String,
    pub(crate) code: Option<String>,
    pub(crate) redirect_uri: Option<String>,
    pub(crate) code_verifier: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) resource: Option<String>,
    pub(crate) scope: Option<String>,
}

pub(crate) fn invalid_grant() -> Response {
    error("invalid_grant", "Grant is invalid, expired or already used")
}

pub(crate) async fn token(State(app): State<App>, Form(e): Form<Exchange>) -> OAuthResult {
    let resource = format!("{}/mcp", base(&app).map_err(IntoResponse::into_response)?);
    if e.resource.as_ref().is_some_and(|r| r != &resource) {
        return Err(error("invalid_target", "Unknown resource"));
    }
    if e.scope.as_deref().is_some_and(|s| s != "joppa") {
        return Err(error("invalid_scope", "Use scope joppa"));
    }
    let mut conn = app.pool.get().await.map_err(internal)?;
    let tx = conn.transaction().await.map_err(internal)?;
    let (actor, family, consumed) = match e.grant_type.as_str() {
        "authorization_code" => {
            let code = e.code.as_deref().ok_or_else(invalid_grant)?;
            let verifier = e.code_verifier.as_deref().ok_or_else(invalid_grant)?;
            if !(43..=128).contains(&verifier.len())
                || !verifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
            {
                return Err(invalid_grant());
            }
            let digest = hash(code);
            let row=tx.query_opt("SELECT actor,challenge,redirect_uri,resource FROM joppa.oauth_codes WHERE hash=$1 AND client_id=$2 AND expires_at>clock_timestamp()",&[&digest,&e.client_id]).await.map_err(internal)?.ok_or_else(invalid_grant)?;
            let challenge: String = row.get(1);
            let redirect: String = row.get(2);
            let target: String = row.get(3);
            if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != challenge
                || e.redirect_uri.as_ref() != Some(&redirect)
                || target != resource
            {
                return Err(invalid_grant());
            }
            (row.get::<_, String>(0), digest.clone(), digest)
        }
        "refresh_token" => {
            let digest = hash(e.refresh_token.as_deref().ok_or_else(invalid_grant)?);
            let row=tx.query_opt("SELECT actor,family FROM joppa.oauth_tokens t WHERE hash=$1 AND client_id=$2 AND resource=$3 AND kind='refresh' AND expires_at>clock_timestamp() AND NOT EXISTS(SELECT 1 FROM joppa.oauth_revoked r WHERE r.family=t.family)",&[&digest,&e.client_id,&resource]).await.map_err(internal)?.ok_or_else(invalid_grant)?;
            (row.get(0), row.get(1), digest)
        }
        _ => {
            return Err(error(
                "unsupported_grant_type",
                "Use authorization_code or refresh_token",
            ));
        }
    };
    let inserted = tx
        .execute(
            "INSERT INTO joppa.oauth_consumed(hash) VALUES($1) ON CONFLICT DO NOTHING",
            &[&consumed],
        )
        .await
        .map_err(internal)?;
    if inserted == 0 {
        // Replaying a refresh token revokes the entire family, including its successor.
        if e.grant_type == "refresh_token" {
            tx.execute(
                "INSERT INTO joppa.oauth_revoked(family) VALUES($1) ON CONFLICT DO NOTHING",
                &[&family],
            )
            .await
            .map_err(internal)?;
            tx.commit().await.map_err(internal)?;
        }
        return Err(invalid_grant());
    }
    let access = format!("jpa_{}", random());
    let refresh = format!("jpr_{}", random());
    for (raw, kind, seconds) in [
        (&access, "access", 3600i32),
        // Тридцать дней — столько же, сколько в ntk. Один срок на оба
        // сервера: разные означали бы, что человек помнит, какой где.
        (&refresh, "refresh", 30 * 24 * 3600),
    ] {
        tx.execute("INSERT INTO joppa.oauth_tokens(hash,kind,actor,client_id,resource,family,expires_at) VALUES($1,$2,$3,$4,$5,$6,clock_timestamp()+make_interval(secs=>$7))",&[&hash(raw),&kind,&actor,&e.client_id,&resource,&family,&(seconds as f64)]).await.map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({"access_token":access,"token_type":"Bearer","expires_in":3600,"refresh_token":refresh,"scope":"joppa"})).into_response())
}

#[derive(Deserialize)]
pub(crate) struct Revocation {
    pub(crate) token: String,
    pub(crate) client_id: String,
}

pub(crate) async fn revoke(State(app): State<App>, Form(r): Form<Revocation>) -> OAuthResult {
    app.pool.get().await.map_err(internal)?.execute("INSERT INTO joppa.oauth_revoked(family) SELECT family FROM joppa.oauth_tokens WHERE hash=$1 AND client_id=$2 ON CONFLICT DO NOTHING",&[&hash(&r.token),&r.client_id]).await.map_err(internal)?;
    Ok(StatusCode::OK.into_response())
}

pub(crate) async fn actor(app: &App, token: &str) -> Result<Option<String>> {
    if !token.starts_with("jpa_") || token.len() != 68 {
        return Ok(None);
    }
    let Some(origin) = &app.oauth_origin else {
        return Ok(None);
    };
    let resource = format!("{origin}/mcp");
    Ok(app.pool.get().await?.query_opt("SELECT actor FROM joppa.oauth_tokens t WHERE hash=$1 AND kind='access' AND resource=$2 AND expires_at>clock_timestamp() AND NOT EXISTS(SELECT 1 FROM joppa.oauth_revoked r WHERE r.family=t.family)",&[&hash(token),&resource]).await?.map(|r|r.get(0)))
}

pub(crate) fn challenge(app: &App) -> Response {
    let mut r=(StatusCode::UNAUTHORIZED,Json(json!({"error":"unauthorized","error_description":"Sign in with your MCP client. Codex CLI: run `codex mcp login joppa` with the same Codex configuration, finish browser sign-in with Allow access, then reconnect MCP. Codex app: MCP server settings > Authenticate. Claude Code or agy: /mcp > joppa > Authenticate. Use the configured server name if renamed. Do not request API keys or implement OAuth with curl."}))).into_response();
    if let Some(base) = &app.oauth_origin {
        r.headers_mut().insert(header::WWW_AUTHENTICATE,format!("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\", scope=\"joppa\", error_description=\"Sign in with your MCP client. Codex CLI: codex mcp login joppa\"").parse().unwrap());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_validation_does_not_confuse_host_prefixes_with_loopback() {
        for u in [
            "https://client.example/callback",
            "http://localhost:3456/callback",
            "http://127.0.0.1:3456/callback",
        ] {
            assert!(valid_redirect(u), "{u}");
        }
        for u in [
            "http://localhost.attacker.example/cb",
            "http://127.0.0.1.attacker.example/cb",
            "https://user:pass@client.example/cb",
            "https://client.example/#frag",
            "javascript:alert(1)",
            "https://client.example/cb?code=bad",
        ] {
            assert!(!valid_redirect(u), "{u}");
        }
    }
}
