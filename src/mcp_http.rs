#![allow(clippy::result_large_err)] // Axum handlers return HTTP responses for both outcomes.

use super::*;
use axum::response::{IntoResponse, Response};
pub(super) fn routes() -> Router<App> {
    Router::new().route("/mcp", post(endpoint).get(get_endpoint))
}
async fn identity(app: &App, headers: &HeaderMap) -> std::result::Result<String, Response> {
    if headers.contains_key(header::ORIGIN) {
        let origin = headers.get(header::ORIGIN).and_then(|s| s.to_str().ok());
        if origin != app.oauth_origin.as_deref() {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Origin not allowed"})),
            )
                .into_response());
        }
    }
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    else {
        return Err(oauth::challenge(app));
    };
    if token.starts_with("jpa_") {
        return match oauth::actor(app, token).await {
            Ok(Some(a)) => Ok(a),
            Ok(None) => Err(oauth::challenge(app)),
            Err(_) => Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Could not check credentials"})),
            )
                .into_response()),
        };
    }
    // Private service credentials remain available to unattended integrations.
    app.identities
        .get(token)
        .cloned()
        .ok_or_else(|| oauth::challenge(app))
}
async fn get_endpoint(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Err(e) = identity(&app, &headers).await {
        return e;
    }
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}
async fn endpoint(
    State(app): State<App>,
    headers: HeaderMap,
    Json(message): Json<Value>,
) -> Response {
    let actor = match identity(&app, &headers).await {
        Ok(a) => a,
        Err(e) => return e,
    };
    if message["jsonrpc"] != "2.0" || !message["method"].is_string() || !message.is_object() {
        return (StatusCode::BAD_REQUEST,Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}))).into_response();
    }
    match crate::protocol::handle(&app.pool, &actor, &message, app.embedder.as_ref()).await {
        Some(r) => Json(r).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}
