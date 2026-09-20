use super::*;
use axum::response::{IntoResponse, Response};
use joppa::device_auth;

pub(super) fn routes() -> Router<App> {
    Router::new()
        .route(
            "/connect",
            get(|| async { Html(include_str!("connect.html")) }),
        )
        .route("/auth/device/start", post(start))
        .route("/auth/device/approve", post(approve))
        .route("/auth/device/poll", post(poll))
        .route("/auth/device/revoke", post(revoke))
        .route("/mcp", post(mcp))
}
async fn start(State(app): State<App>) -> ApiResult {
    result(app.devices.start())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    code: String,
}
async fn approve(State(app): State<App>, headers: HeaderMap, Json(a): Json<Approval>) -> ApiResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    // Device authorization is a human browser action, never a bearer-key action.
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Approve this connection in your browser"));
    }
    let actor = authenticate(&app, &headers).await?;
    let spaces = store::list(&app.pool, &actor).await.map_err(auth_error)?;
    if spaces.as_array().is_none_or(|s| s.is_empty()) {
        return Err(auth_error("Your account has no workspace access"));
    }
    result(
        app.devices
            .approve(&a.code, &actor)
            .map(|()| json!({"ok":true})),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Poll {
    code: String,
    device_secret: String,
}
async fn poll(State(app): State<App>, Json(p): Json<Poll>) -> ApiResult {
    result(async {
        match app.devices.poll(&p.code,&p.device_secret)? {
            None=>Ok(json!({"status":"pending"})),
            Some(actor)=>Ok(json!({"status":"ready","key":device_auth::issue(&app.pool,&actor).await?,"expires_in":90*24*3600})),
        }
    }.await)
}
async fn revoke(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| auth_error("CLI key required"))?;
    if device_auth::actor(&app.pool, token)
        .await
        .map_err(auth_error)?
        .is_none()
    {
        return Err(auth_error("Valid CLI key required"));
    }
    result(
        device_auth::revoke(&app.pool, token)
            .await
            .map(|()| json!({"ok":true})),
    )
}
async fn mcp(State(app): State<App>, headers: HeaderMap, Json(message): Json<Value>) -> Response {
    if !headers.contains_key(header::AUTHORIZATION) {
        return auth_error("Use your CLI key to connect MCP").into_response();
    }
    let actor = match authenticate(&app, &headers).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    match crate::protocol::handle(&app.pool, &actor, &message).await {
        Some(response) => Json(response).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}
