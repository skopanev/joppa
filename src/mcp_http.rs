#![allow(clippy::result_large_err)] // Axum handlers return HTTP responses for both outcomes.

use super::*;
use axum::response::{IntoResponse, Response};
pub(super) fn routes() -> Router<App> {
    // joppa_attach carries a file of up to 50 MiB in base64.
    let limit = axum::extract::DefaultBodyLimit::max(joppa::spaces::REQUEST_MAX);
    Router::new().route("/mcp", post(endpoint).layer(limit).get(get_endpoint))
}
/// The actor of a call and the workspace of a write key: an OAuth identity, a workspace write
/// key, or a private service credential.
async fn identity(
    app: &App,
    headers: &HeaderMap,
) -> std::result::Result<(String, Option<String>), Response> {
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
            Ok(Some(a)) => Ok((a, None)),
            Ok(None) => Err(oauth::challenge(app)),
            Err(_) => Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Could not check credentials"})),
            )
                .into_response()),
        };
    }
    if token.starts_with(joppa::write_key::KEY_PREFIX) {
        if !joppa::write_key::is_write_key(token) {
            return Err(oauth::challenge(app));
        }
        let digest = oauth::hash(token);
        return match store::write_key_identity(&app.pool, &digest).await {
            Ok(Some((workspace, actor))) => Ok((actor, Some(workspace))),
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
        .map(|a| (a, None))
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
    let (actor, key_workspace) = match identity(&app, &headers).await {
        Ok(found) => found,
        Err(e) => return e,
    };
    if message["jsonrpc"] != "2.0" || !message["method"].is_string() || !message.is_object() {
        return (StatusCode::BAD_REQUEST,Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}))).into_response();
    }
    let alias = headers
        .get(joppa::actor_alias::HEADER)
        .and_then(|v| v.to_str().ok());
    let work = crate::protocol::handle(&app.pool, &actor, &message, app.embedder.as_ref(), alias);
    let start = std::time::Instant::now();
    let reply = joppa::write_key::scope(key_workspace.clone(), work).await;
    let ms = start.elapsed().as_millis();
    if ms >= super::SLOW_REQUEST_MS {
        // The log names the call, never its arguments or a token.
        let params = &message["params"];
        let method = message["method"].as_str().unwrap_or("-");
        let tool = params["name"].as_str().unwrap_or("-");
        let workspace = params["arguments"]["workspace"].as_str();
        let workspace = workspace.or(key_workspace.as_deref()).unwrap_or("-");
        let status = match &reply {
            Some(r) if r.get("error").is_some() => "error",
            Some(_) => "ok",
            None => "accepted",
        };
        eprintln!(
            "joppa: slow mcp method={method} tool={tool} workspace={workspace} actor={actor} status={status} ms={ms}"
        );
    }
    match reply {
        Some(r) => Json(r).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}
