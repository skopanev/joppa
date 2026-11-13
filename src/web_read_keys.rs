//! The read key routes of the Settings page. Only a signed-in browser session of a human
//! member manages the key. A request with an Authorization header gets 401, also a request
//! with a read key.
use super::*;

/// Refuse an Authorization header with 401 and a foreign Origin with 403. A write needs the
/// same-origin Origin header. A browser sends no Origin on a same-origin GET.
fn browser_only(
    headers: &HeaderMap,
    write: bool,
) -> std::result::Result<(), (StatusCode, Json<Value>)> {
    if headers.contains_key(header::AUTHORIZATION) {
        let error = json!({"error":"Sign in with the browser to manage the read key"});
        return Err((StatusCode::UNAUTHORIZED, Json(error)));
    }
    if write || headers.contains_key(header::ORIGIN) {
        web_auth::check_origin(headers).map_err(auth_error)?;
    }
    Ok(())
}

/// A person who is not a human member of the workspace gets 403.
fn keys_result(value: Result<Value>) -> ApiResult {
    value
        .map(Json)
        .map_err(|e| match e.is::<store::AccessDenied>() {
            true => auth_error("a human member of the workspace is required"),
            false => failure(e),
        })
}

/// The signed-in actor of a read key route.
async fn member(
    app: &App,
    headers: &HeaderMap,
    write: bool,
) -> std::result::Result<String, (StatusCode, Json<Value>)> {
    browser_only(headers, write)?;
    authenticate(app, headers).await
}

/// `GET /api/workspaces/{workspace}/read-key`: `{"active", "created_at"}` and never the key.
pub(super) async fn read_key(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = member(&app, &headers, false).await?;
    keys_result(store::read_key(&app.pool, &workspace, &actor).await)
}

/// `POST /api/workspaces/{workspace}/read-key`: a new key replaces the previous one. The
/// reply gives the key once. The server keeps only its digest.
pub(super) async fn create_read_key(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = member(&app, &headers, true).await?;
    let key = format!("{}{}", joppa::data_request::KEY_PREFIX, oauth::random());
    let digest = oauth::hash(&key);
    let created = store::set_read_key(&app.pool, &workspace, &actor, &digest).await;
    keys_result(created.map(|created_at| json!({"key": key, "created_at": created_at})))
}

/// `POST /api/workspaces/{workspace}/read-key/revoke`: the key stops at once.
pub(super) async fn revoke_read_key(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = member(&app, &headers, true).await?;
    keys_result(store::revoke_read_key(&app.pool, &workspace, &actor).await)
}
