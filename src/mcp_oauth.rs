//! Browser consent and OAuth authorization-code/PKCE for direct HTTPS MCP clients.
use super::*;
use axum::{
    Form,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use sha2::{Digest, Sha256};

type OAuthResult = std::result::Result<Response, Response>;
fn error(code: &str, description: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":code,"error_description":description})),
    )
        .into_response()
}
fn internal(_: impl std::fmt::Display) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"temporarily_unavailable"})),
    )
        .into_response()
}
fn invalid(_: impl std::fmt::Display) -> Response {
    error("invalid_request", "Invalid authorization request")
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn base(app: &App) -> std::result::Result<&str, (StatusCode, Json<Value>)> {
    app.oauth_origin.as_deref().ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"temporarily_unavailable","error_description":"Public server URL is not configured"}))))
}
pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(resource))
        .route("/.well-known/oauth-protected-resource/mcp", get(resource))
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/details", get(details))
        .route("/oauth/approve", post(approve))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
}
async fn resource(State(app): State<App>) -> OAuthResult {
    let base = base(&app).map_err(IntoResponse::into_response)?;
    Ok(Json(json!({"resource":format!("{base}/mcp"),"authorization_servers":[base],"bearer_methods_supported":["header"],"scopes_supported":["joppa"]})).into_response())
}
async fn metadata(State(app): State<App>) -> OAuthResult {
    let b = base(&app).map_err(IntoResponse::into_response)?;
    Ok(Json(json!({"issuer":b,"authorization_endpoint":format!("{b}/oauth/authorize"),"token_endpoint":format!("{b}/oauth/token"),"registration_endpoint":format!("{b}/oauth/register"),"revocation_endpoint":format!("{b}/oauth/revoke"),"scopes_supported":["joppa"],"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none"],"code_challenge_methods_supported":["S256"]})).into_response())
}
#[derive(Deserialize)]
struct Registration {
    client_name: Option<String>,
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
}
fn valid_redirect(raw: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(raw) else {
        return false;
    };
    (u.scheme() == "https"
        || (u.scheme() == "http"
            && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
        && u.host_str().is_some()
        && u.username().is_empty()
        && u.password().is_none()
        && u.fragment().is_none()
        && raw.len() <= 2048
        && !u
            .query_pairs()
            .any(|(k, _)| matches!(k.as_ref(), "code" | "state" | "error"))
}
async fn register(State(app): State<App>, Json(r): Json<Registration>) -> OAuthResult {
    if r.redirect_uris.is_empty()
        || r.redirect_uris.len() > 10
        || !r.redirect_uris.iter().all(|u| valid_redirect(u))
    {
        return Err(error(
            "invalid_redirect_uri",
            "Use HTTPS or a loopback HTTP callback",
        ));
    }
    if r.token_endpoint_auth_method
        .as_deref()
        .is_some_and(|s| s != "none")
        || r.grant_types.as_ref().is_some_and(|g| {
            g.iter()
                .any(|s| !matches!(s.as_str(), "authorization_code" | "refresh_token"))
        })
        || r.response_types
            .as_ref()
            .is_some_and(|g| g.iter().any(|s| s != "code"))
    {
        return Err(error(
            "invalid_client_metadata",
            "Use authorization_code with PKCE and public client authentication",
        ));
    }
    let name = r.client_name.unwrap_or_else(|| "MCP client".into());
    if name.trim().is_empty() || name.len() > 200 {
        return Err(error("invalid_client_metadata", "Invalid client name"));
    }
    let id = format!("jpc_{}", random());
    app.pool
        .get()
        .await
        .map_err(internal)?
        .execute(
            "INSERT INTO joppa.oauth_clients(client_id,name,redirect_uris) VALUES($1,$2,$3)",
            &[&id, &name, &r.redirect_uris],
        )
        .await
        .map_err(internal)?;
    Ok((StatusCode::CREATED,Json(json!({"client_id":id,"client_name":name,"redirect_uris":r.redirect_uris,"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]}))).into_response())
}
#[derive(Deserialize, Serialize)]
struct Authorization {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    code_challenge: String,
    code_challenge_method: String,
    state: Option<String>,
    scope: Option<String>,
    resource: Option<String>,
}
async fn validate(app: &App, a: &Authorization) -> std::result::Result<String, Response> {
    if a.response_type != "code"
        || a.code_challenge_method != "S256"
        || URL_SAFE_NO_PAD
            .decode(&a.code_challenge)
            .map_or(true, |b| b.len() != 32)
        || a.state.as_ref().is_some_and(|s| s.len() > 2048)
        || a.scope.as_deref().is_some_and(|s| s != "joppa")
    {
        return Err(error(
            "invalid_request",
            "Use code, S256 PKCE and scope joppa",
        ));
    }
    let resource = format!("{}/mcp", base(app).map_err(IntoResponse::into_response)?);
    // Older clients omit resource; this authorization server has exactly one resource.
    if a.resource.as_ref().is_some_and(|r| r != &resource) {
        return Err(error("invalid_target", "Unknown resource"));
    }
    let c = app.pool.get().await.map_err(internal)?;
    let row = c
        .query_opt(
            "SELECT name FROM joppa.oauth_clients WHERE client_id=$1 AND $2=ANY(redirect_uris)",
            &[&a.client_id, &a.redirect_uri],
        )
        .await
        .map_err(internal)?;
    row.map(|r| r.get(0))
        .ok_or_else(|| error("invalid_client", "Unknown client or callback"))
}
async fn authorize(State(app): State<App>, Query(a): Query<Authorization>) -> OAuthResult {
    validate(&app, &a).await?;
    Ok((
        [
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_FRAME_OPTIONS, "DENY"),
        ],
        Html(include_str!("oauth.html")),
    )
        .into_response())
}
// Antigravity's native CLI asks the user to paste a code. Its hosted callback
// only displays that code; keep this manual completion on our own origin.
fn manual_callback(uri: &str) -> bool {
    matches!(
        uri,
        "https://antigravity.google/oauth-callback" | "https://antigravity.google/oauth-callback/"
    )
}
fn copy_link_callback(uri: &str) -> bool {
    reqwest::Url::parse(uri)
        .is_ok_and(|url| matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
}
async fn details(State(app): State<App>, Query(a): Query<Authorization>) -> OAuthResult {
    let name = validate(&app, &a).await?;
    Ok(Json(json!({"client_name":name,"redirect_uri":a.redirect_uri,"manual":manual_callback(&a.redirect_uri),"copy_link":copy_link_callback(&a.redirect_uri)})).into_response())
}
async fn approve(
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
struct Exchange {
    grant_type: String,
    client_id: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    resource: Option<String>,
    scope: Option<String>,
}
fn invalid_grant() -> Response {
    error("invalid_grant", "Grant is invalid, expired or already used")
}
async fn token(State(app): State<App>, Form(e): Form<Exchange>) -> OAuthResult {
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
        (&refresh, "refresh", 90 * 24 * 3600),
    ] {
        tx.execute("INSERT INTO joppa.oauth_tokens(hash,kind,actor,client_id,resource,family,expires_at) VALUES($1,$2,$3,$4,$5,$6,clock_timestamp()+make_interval(secs=>$7))",&[&hash(raw),&kind,&actor,&e.client_id,&resource,&family,&(seconds as f64)]).await.map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({"access_token":access,"token_type":"Bearer","expires_in":3600,"refresh_token":refresh,"scope":"joppa"})).into_response())
}
#[derive(Deserialize)]
struct Revocation {
    token: String,
    client_id: String,
}
async fn revoke(State(app): State<App>, Form(r): Form<Revocation>) -> OAuthResult {
    app.pool.get().await.map_err(internal)?.execute("INSERT INTO joppa.oauth_revoked(family) SELECT family FROM joppa.oauth_tokens WHERE hash=$1 AND client_id=$2 ON CONFLICT DO NOTHING",&[&hash(&r.token),&r.client_id]).await.map_err(internal)?;
    Ok(StatusCode::OK.into_response())
}
pub(super) async fn actor(app: &App, token: &str) -> Result<Option<String>> {
    if !token.starts_with("jpa_") || token.len() != 68 {
        return Ok(None);
    }
    let Some(origin) = &app.oauth_origin else {
        return Ok(None);
    };
    let resource = format!("{origin}/mcp");
    Ok(app.pool.get().await?.query_opt("SELECT actor FROM joppa.oauth_tokens t WHERE hash=$1 AND kind='access' AND resource=$2 AND expires_at>clock_timestamp() AND NOT EXISTS(SELECT 1 FROM joppa.oauth_revoked r WHERE r.family=t.family)",&[&hash(token),&resource]).await?.map(|r|r.get(0)))
}
pub(super) fn challenge(app: &App) -> Response {
    let mut r=(StatusCode::UNAUTHORIZED,Json(json!({"error":"unauthorized","error_description":"Sign in with your MCP client. Codex CLI: run `codex mcp login joppa` with the same Codex configuration, finish browser sign-in with Return to app, then reconnect MCP. Codex app: MCP server settings > Authenticate. Claude Code or agy: /mcp > joppa > Authenticate. Use the configured server name if renamed. Do not request API keys or implement OAuth with curl."}))).into_response();
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
