use crate::web_auth;
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Html,
    routing::{get, post},
};
use deadpool_postgres::Pool;
use joppa::{commands::Command, sessions, store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
#[path = "mcp_http.rs"]
mod mcp;
#[path = "mcp_oauth.rs"]
mod oauth;

#[derive(Clone)]
struct App {
    pool: Pool,
    identities: Arc<BTreeMap<String, String>>,
    login: web_auth::Login,
    oauth_origin: Option<String>,
    embedder: Option<joppa::embeddings::Embedder>,
}
type ApiResult = std::result::Result<Json<Value>, (StatusCode, Json<Value>)>;
async fn no_store(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
/// A read grant never changes state. Every API request other than GET or HEAD,
/// to a known route or not, is refused before the route reads or validates the body.
/// Other credentials pass; the route checks them as before.
async fn refuse_read_writes(
    State(app): State<App>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::{http::Method, response::IntoResponse};
    let write = !matches!(*request.method(), Method::GET | Method::HEAD);
    if write && request.uri().path().starts_with("/api/") {
        let token = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "));
        if let Some(token) = token
            && !app.identities.contains_key(token)
            && let Ok(Some(grant)) = store::token_actor(&app.pool, token).await
            && let Err(refusal) = refuse_read_grant(grant.access)
        {
            return refusal.into_response();
        }
    }
    next.run(request).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request_id: String,
    command: Value,
}
async fn authenticate(
    app: &App,
    headers: &HeaderMap,
) -> std::result::Result<String, (StatusCode, Json<Value>)> {
    authenticate_grant(app, headers)
        .await
        .map(|(actor, _)| actor)
}
/// Routes that change state call this. refuse_read_writes already refuses a read grant on
/// /api/ writes; this is the second guard.
async fn authenticate_write(
    app: &App,
    headers: &HeaderMap,
) -> std::result::Result<String, (StatusCode, Json<Value>)> {
    let (actor, access) = authenticate_grant(app, headers).await?;
    refuse_read_grant(access)?;
    Ok(actor)
}
fn refuse_read_grant(access: store::Access) -> std::result::Result<(), (StatusCode, Json<Value>)> {
    if access == store::Access::Read {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": store::READ_ONLY_REFUSAL})),
        ));
    }
    Ok(())
}
/// The actor and its access grant. Service credentials and browser sessions write;
/// the browser write restriction is require_write_credential.
async fn authenticate_grant(
    app: &App,
    headers: &HeaderMap,
) -> std::result::Result<(String, store::Access), (StatusCode, Json<Value>)> {
    let actor = if headers.contains_key(header::AUTHORIZATION) {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "));
        match token {
            Some(t) => match app.identities.get(t) {
                Some(actor) => Some((actor.clone(), store::Access::Write)),
                None => match store::token_actor(&app.pool, t).await {
                    Ok(grant) => grant.map(|g| (g.agent, g.access)),
                    Err(e) => {
                        return Err((
                            StatusCode::UNAUTHORIZED,
                            Json(json!({"error": e.to_string()})),
                        ));
                    }
                },
            },
            None => None,
        }
    } else if let Some(token) = web_auth::cookie(headers, "joppa_session") {
        let session = sessions::actor(&app.pool, token).await.map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Could not check your session. Please retry."})),
            )
        })?;
        // A grant made after sign-in takes effect on the next page load, without a new sign-in.
        // A domain grant needs the hd of this session's own sign-in.
        if let Some((actor, hd)) = &session {
            let configured = app.login.users.iter().find(|(_, id)| *id == actor);
            let configured = configured.map(|(email, _)| email.as_str());
            // A grant fault answers a fixed error; the server log keeps the cause.
            store::enroll_granted(&app.pool, actor, configured, hd.as_deref())
                .await
                .map_err(|e| {
                    if e.is::<store::AccessDenied>() {
                        return failure(e);
                    }
                    eprintln!("joppa: sign-in grants failed: {e:#}");
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":"sign-in grants are not available, try again"})),
                    )
                })?;
        }
        session.map(|(actor, _)| (actor, store::Access::Write))
    } else {
        None
    };
    actor.ok_or((
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":"Please sign in to continue"})),
    ))
}
fn result(value: Result<Value>) -> ApiResult {
    value.map(Json).map_err(failure)
}
fn failure(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    // A database fault stays in the server log; the browser gets no database text.
    if e.chain()
        .any(|cause| cause.downcast_ref::<tokio_postgres::Error>().is_some())
    {
        eprintln!("joppa: request failed: {e:#}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"The server could not complete the request. Try again later."})),
        );
    }
    (
        if e.is::<store::AccessDenied>() {
            StatusCode::NOT_FOUND
        } else if e.is::<store::ProjectionNotReady>() {
            StatusCode::SERVICE_UNAVAILABLE
        } else if e.is::<joppa::commands::ClientField>() || e.is::<joppa::objects::RequiredField>()
        {
            StatusCode::UNPROCESSABLE_ENTITY
        } else {
            StatusCode::CONFLICT
        },
        Json(json!({"error":format!("{e:#}")})),
    )
}
/// Cookie sessions pass the shared origin check; bearer calls carry no ambient credential.
fn check_session_origin(headers: &HeaderMap) -> std::result::Result<(), (StatusCode, Json<Value>)> {
    if headers.contains_key(header::AUTHORIZATION) {
        return Ok(());
    }
    web_auth::check_origin(headers).map_err(auth_error)
}
/// Browser sessions can read product data; writes require agent/service credentials.
fn require_write_credential(
    headers: &HeaderMap,
) -> std::result::Result<(), (StatusCode, Json<Value>)> {
    if !headers.contains_key(header::AUTHORIZATION) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error":"The portal is read-only. Use Joppa MCP to make changes."})),
        ));
    }
    Ok(())
}

#[path = "web_files.rs"]
mod files;
use files::*;
#[path = "web_workspaces.rs"]
mod workspaces;
use workspaces::*;
#[path = "web_reads.rs"]
mod reads;
use reads::*;
#[path = "web_search.rs"]
mod search_route;
use search_route::*;
#[path = "web_reports.rs"]
mod reports;
use reports::*;
#[path = "web_commands.rs"]
mod commands;
use commands::*;
#[path = "web_login.rs"]
mod login;
use login::*;
#[path = "web_server.rs"]
mod server;
pub use server::serve;
#[path = "web_board.rs"]
mod board_script;
use board_script::*;
#[path = "web_pages.rs"]
mod pages;
pub use pages::snapshot;
use pages::*;

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
