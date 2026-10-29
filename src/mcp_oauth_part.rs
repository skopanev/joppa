//! Part of `mcp_oauth`: `part` and the items after it, moved from mcp_oauth.rs unchanged.
use super::*;

pub(crate) type OAuthResult = std::result::Result<Response, Response>;

pub(crate) fn error(code: &str, description: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":code,"error_description":description})),
    )
        .into_response()
}

pub(crate) fn internal(_: impl std::fmt::Display) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"temporarily_unavailable"})),
    )
        .into_response()
}

pub(crate) fn invalid(_: impl std::fmt::Display) -> Response {
    error("invalid_request", "Invalid authorization request")
}

pub(crate) fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub(crate) fn hash(s: &str) -> String {
    joppa::digest_hex::sha256_hex(s.as_bytes())
}

pub(crate) fn base(app: &App) -> std::result::Result<&str, (StatusCode, Json<Value>)> {
    app.oauth_origin.as_deref().ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"temporarily_unavailable","error_description":"Public server URL is not configured"}))))
}

pub(crate) fn routes() -> Router<App> {
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

pub(crate) async fn resource(State(app): State<App>) -> OAuthResult {
    let base = base(&app).map_err(IntoResponse::into_response)?;
    Ok(Json(json!({"resource":format!("{base}/mcp"),"authorization_servers":[base],"bearer_methods_supported":["header"],"scopes_supported":["joppa"]})).into_response())
}

pub(crate) async fn metadata(State(app): State<App>) -> OAuthResult {
    let b = base(&app).map_err(IntoResponse::into_response)?;
    Ok(Json(json!({"issuer":b,"authorization_endpoint":format!("{b}/oauth/authorize"),"token_endpoint":format!("{b}/oauth/token"),"registration_endpoint":format!("{b}/oauth/register"),"revocation_endpoint":format!("{b}/oauth/revoke"),"scopes_supported":["joppa"],"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none"],"code_challenge_methods_supported":["S256"]})).into_response())
}

#[derive(Deserialize)]
pub(crate) struct Registration {
    pub(crate) client_name: Option<String>,
    pub(crate) redirect_uris: Vec<String>,
    pub(crate) token_endpoint_auth_method: Option<String>,
    pub(crate) grant_types: Option<Vec<String>>,
    pub(crate) response_types: Option<Vec<String>>,
}

pub(crate) fn valid_redirect(raw: &str) -> bool {
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

pub(crate) async fn register(State(app): State<App>, Json(r): Json<Registration>) -> OAuthResult {
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
pub(crate) struct Authorization {
    pub(crate) client_id: String,
    pub(crate) redirect_uri: String,
    pub(crate) response_type: String,
    pub(crate) code_challenge: String,
    pub(crate) code_challenge_method: String,
    pub(crate) state: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) resource: Option<String>,
}

pub(crate) async fn validate(
    app: &App,
    a: &Authorization,
) -> std::result::Result<String, Response> {
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

pub(crate) async fn authorize(
    State(app): State<App>,
    Query(a): Query<Authorization>,
) -> OAuthResult {
    validate(&app, &a).await?;
    Ok((
        [
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_FRAME_OPTIONS, "DENY"),
        ],
        Html(
            include_str!("oauth.html")
                .replace("/* BOARD_STYLE */", include_str!("board.css"))
                .replace("/* BRAND_STYLE */", include_str!("brand.css")),
        ),
    )
        .into_response())
}

// Antigravity's native CLI asks the user to paste a code. Its hosted callback
// only displays that code; keep this manual completion on our own origin.
pub(crate) fn manual_callback(uri: &str) -> bool {
    matches!(
        uri,
        "https://antigravity.google/oauth-callback" | "https://antigravity.google/oauth-callback/"
    )
}

pub(crate) fn copy_link_callback(uri: &str) -> bool {
    reqwest::Url::parse(uri)
        .is_ok_and(|url| matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
}

pub(crate) async fn details(State(app): State<App>, Query(a): Query<Authorization>) -> OAuthResult {
    let name = validate(&app, &a).await?;
    Ok(Json(json!({"client_name":name,"redirect_uri":a.redirect_uri,"manual":manual_callback(&a.redirect_uri),"copy_link":copy_link_callback(&a.redirect_uri)})).into_response())
}
