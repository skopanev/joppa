//! Browser sign-in: the session, Google login, polling and logout.
use super::*;

pub(super) async fn me(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        async {
            let conn = app.pool.get().await?;
            let identity = conn
                .query_opt(
                    "SELECT email,display_name FROM joppa.google_identities WHERE actor=$1",
                    &[&actor],
                )
                .await?;
            let email: Option<String> = identity.as_ref().map(|row| row.get(0));
            let name: Option<String> = identity.as_ref().and_then(|row| row.get(1));
            Ok(json!({"actor":actor,"email":email,"name":name}))
        }
        .await,
    )
}
pub(super) type AuthResult =
    std::result::Result<(HeaderMap, Json<Value>), (StatusCode, Json<Value>)>;
pub(super) fn auth_error(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (StatusCode::FORBIDDEN, Json(json!({"error":e.to_string()})))
}
pub(super) fn auth_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h
}
pub(super) async fn login_user(State(app): State<App>, headers: HeaderMap) -> AuthResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let (id, data) = app
        .login
        .start(web_auth::cookie(&headers, "joppa_login"))
        .await
        .map_err(auth_error)?;
    let mut response = auth_headers();
    response.insert(
        header::SET_COOKIE,
        web_auth::set_cookie(&headers, "joppa_login", &id, 300)
            .replace("SameSite=Strict", "SameSite=Lax")
            .parse()
            .unwrap(),
    );
    Ok((response, Json(data)))
}
#[derive(Deserialize)]
pub(super) struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
pub(super) async fn google_callback(
    State(app): State<App>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> (HeaderMap, Html<String>) {
    let result = match (
        web_auth::cookie(&headers, "joppa_login"),
        q.state.as_deref(),
    ) {
        (Some(id), Some(state)) => {
            app.login
                .callback(&app.pool, id, state, q.code.as_deref(), q.error.is_some())
                .await
        }
        _ => Err(anyhow::anyhow!("Missing sign-in state")),
    };
    let message = if result.is_ok() {
        "Return to Joppa to finish signing in. You can close this window."
    } else {
        "This sign-in has expired or belongs to another browser. Return to Joppa and try again."
    };
    let mut h = auth_headers();
    h.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    (
        h,
        Html(format!(
            "<!doctype html><html lang=\"en\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>joppa</title><link rel=\"icon\" type=\"image/svg+xml\" href=\"/favicon.svg\"><style>body{{font:16px/1.6 system-ui;background:#f5f5f0;color:#263729;display:grid;place-items:center;min-height:90vh;margin:0}}main{{max-width:360px;padding:32px}}h1{{letter-spacing:-.05em;font-size:40px}}</style><main><h1>joppa.</h1><p>{message}</p></main></html>"
        )),
    )
}
pub(super) async fn poll_login(State(app): State<App>, headers: HeaderMap) -> AuthResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let id = web_auth::cookie(&headers, "joppa_login")
        .ok_or_else(|| auth_error("Start sign-in first"))?;
    let mut response = auth_headers();
    match app.login.poll(id).await.map_err(auth_error)? {
        None => Ok((response, Json(json!({"status":"pending"})))),
        Some((token, actor, hd)) => {
            sessions::create(&app.pool, &token, &actor, hd.as_deref())
                .await
                .map_err(auth_error)?;
            if let Some(old) = web_auth::cookie(&headers, "joppa_session") {
                sessions::revoke(&app.pool, old).await.map_err(auth_error)?;
            }
            response.append(
                header::SET_COOKIE,
                web_auth::set_cookie(
                    &headers,
                    "joppa_session",
                    &token,
                    sessions::LIFETIME_SECONDS,
                )
                .parse()
                .unwrap(),
            );
            response.append(
                header::SET_COOKIE,
                web_auth::set_cookie(&headers, "joppa_login", "", 0)
                    .parse()
                    .unwrap(),
            );
            Ok((response, Json(json!({"status":"ready", "actor":actor}))))
        }
    }
}
pub(super) async fn logout(State(app): State<App>, headers: HeaderMap) -> AuthResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    if let Some(token) = web_auth::cookie(&headers, "joppa_session") {
        sessions::revoke(&app.pool, token)
            .await
            .map_err(auth_error)?;
    }
    let mut response = auth_headers();
    response.insert(
        header::SET_COOKIE,
        web_auth::set_cookie(&headers, "joppa_session", "", 0)
            .parse()
            .unwrap(),
    );
    Ok((response, Json(json!({"ok":true}))))
}
