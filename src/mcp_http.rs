#![allow(clippy::result_large_err)] // Axum handlers return HTTP responses for both outcomes.

use super::*;
use axum::response::{IntoResponse, Response};
pub(super) fn routes() -> Router<App> {
    // joppa_attach carries a file of up to 50 MiB in base64.
    let limit = axum::extract::DefaultBodyLimit::max(joppa::spaces::REQUEST_MAX);
    Router::new().route("/mcp", post(endpoint).layer(limit).get(get_endpoint))
}
/// The actor and its access grant. Service credentials and OAuth identities write.
async fn identity(
    app: &App,
    headers: &HeaderMap,
) -> std::result::Result<(String, store::Access), Response> {
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
            Ok(Some(a)) => Ok((a, store::Access::Write)),
            Ok(None) => Err(oauth::challenge(app)),
            Err(_) => Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Could not check credentials"})),
            )
                .into_response()),
        };
    }
    // Private service credentials remain available to unattended integrations.
    if let Some(actor) = app.identities.get(token) {
        return Ok((actor.clone(), store::Access::Write));
    }
    // Agent tokens that the workspace owner created; a revoked one is refused.
    match store::token_actor(&app.pool, token).await {
        Ok(Some(grant)) => Ok((grant.agent, grant.access)),
        Ok(None) => Err(oauth::challenge(app)),
        Err(e) => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": e.to_string()})),
        )
            .into_response()),
    }
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
    let (actor, access) = match identity(&app, &headers).await {
        Ok(a) => a,
        Err(e) => return e,
    };
    if message["jsonrpc"] != "2.0" || !message["method"].is_string() || !message.is_object() {
        return (StatusCode::BAD_REQUEST,Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}))).into_response();
    }
    match crate::protocol::handle(&app.pool, &actor, access, &message, app.embedder.as_ref()).await
    {
        Some(r) => Json(r).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}
