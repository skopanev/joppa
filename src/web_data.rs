//! `POST /api/data`: the only route of a workspace read key. Each other route refuses the key.
//! A missing, malformed, unknown or revoked key gets 401. The route never writes.
use super::*;
use axum::response::{IntoResponse, Response};
use joppa::data_request::{Refusal, Request, is_read_key};

fn reply(status: StatusCode, error: impl std::fmt::Display) -> Response {
    (status, Json(json!({"error": error.to_string()}))).into_response()
}

/// The workspace of the read key of the request.
async fn key_workspace(app: &App, headers: &HeaderMap) -> std::result::Result<String, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|t| is_read_key(t));
    let refused = || {
        reply(
            StatusCode::UNAUTHORIZED,
            "a valid workspace read key is required",
        )
    };
    let token = token.ok_or_else(refused)?;
    let digest = joppa::digest_hex::sha256_hex(token);
    match store::read_key_workspace(&app.pool, &digest).await {
        Ok(Some(workspace)) => Ok(workspace),
        Ok(None) => Err(refused()),
        Err(e) => Err(failure(e).into_response()),
    }
}

pub(super) async fn data(
    State(app): State<App>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let workspace = match key_workspace(&app, &headers).await {
        Ok(workspace) => workspace,
        Err(refusal) => return refusal,
    };
    let request: Request = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(e) => return reply(StatusCode::BAD_REQUEST, e),
    };
    let embedder = app.embedder.as_ref();
    match store::read_data(&app.pool, &workspace, request, embedder).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => match e.downcast_ref::<Refusal>() {
            Some(r) => reply(
                StatusCode::from_u16(r.status).unwrap_or(StatusCode::BAD_REQUEST),
                r,
            ),
            None => failure(e).into_response(),
        },
    }
}
