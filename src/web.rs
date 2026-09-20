use crate::web_auth;
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::Html,
    routing::{get, post},
};
use deadpool_postgres::Pool;
use joppa::{commands::Command, store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
struct App {
    pool: Pool,
    identities: Arc<BTreeMap<String, String>>,
    login: web_auth::Login,
}
type ApiResult = std::result::Result<Json<Value>, (StatusCode, Json<Value>)>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request_id: String,
    command: Command,
}
fn authenticate(
    app: &App,
    headers: &HeaderMap,
) -> std::result::Result<String, (StatusCode, Json<Value>)> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .and_then(|t| app.identities.get(t))
        .cloned()
        .or_else(|| {
            if headers.contains_key(header::AUTHORIZATION) {
                return None;
            }
            web_auth::cookie(headers, "joppa_session").and_then(|t| app.login.actor(t))
        })
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Please sign in to continue"})),
        ))
}
fn result(value: Result<Value>) -> ApiResult {
    value.map(Json).map_err(|e| {
        (
            StatusCode::CONFLICT,
            Json(json!({"error":format!("{e:#}")})),
        )
    })
}
async fn list(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers)?;
    result(store::list(&app.pool, &actor).await)
}
async fn read(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers)?;
    result(store::read(&app.pool, &workspace, &actor).await)
}
async fn command(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> ApiResult {
    let actor = authenticate(&app, &headers)?;
    if !headers.contains_key(header::AUTHORIZATION) {
        web_auth::check_origin(&headers).map_err(|_| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"cross-origin request denied"})),
            )
        })?;
    }
    result(
        store::execute(
            &app.pool,
            &workspace,
            &actor,
            &request.request_id,
            &request.command,
        )
        .await,
    )
}
async fn me(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers)?;
    Ok(Json(json!({"actor":actor})))
}
type AuthResult = std::result::Result<(HeaderMap, Json<Value>), (StatusCode, Json<Value>)>;
fn auth_error(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (StatusCode::FORBIDDEN, Json(json!({"error":e.to_string()})))
}
fn auth_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h
}
async fn login_user(State(app): State<App>, headers: HeaderMap) -> AuthResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let (id, data) = app
        .login
        .start(web_auth::cookie(&headers, "joppa_login"))
        .await
        .map_err(auth_error)?;
    let mut response = auth_headers();
    response.insert(
        header::SET_COOKIE,
        web_auth::set_cookie(&headers, "joppa_login", &id, 600)
            .parse()
            .unwrap(),
    );
    Ok((response, Json(data)))
}
async fn poll_login(State(app): State<App>, headers: HeaderMap) -> AuthResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let id = web_auth::cookie(&headers, "joppa_login")
        .ok_or_else(|| auth_error("Start sign-in first"))?;
    let mut response = auth_headers();
    match app.login.poll(id).await.map_err(auth_error)? {
        None => Ok((response, Json(json!({"status":"pending"})))),
        Some((token, actor)) => {
            if let Some(old) = web_auth::cookie(&headers, "joppa_session") {
                app.login.logout(old);
            }
            response.append(
                header::SET_COOKIE,
                web_auth::set_cookie(&headers, "joppa_session", &token, 43200)
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
async fn logout(State(app): State<App>, headers: HeaderMap) -> AuthResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    if let Some(token) = web_auth::cookie(&headers, "joppa_session") {
        app.login.logout(token);
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
pub async fn serve(pool: Pool, host: &str, port: u16) -> Result<()> {
    let path = std::env::var("JOPPA_TOKENS_FILE").map_err(|_| {
        anyhow::anyhow!(
            "set JOPPA_TOKENS_FILE to a private JSON file mapping bearer tokens to actor IDs"
        )
    })?;
    let identities: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(path)?)?;
    ensure!(!identities.is_empty(), "identity file is empty");
    for (token, actor) in &identities {
        ensure!(token.len() >= 24, "tokens must have at least 24 characters");
        joppa::commands::identifier(actor)?;
    }
    let login = web_auth::Login::from_env()?;
    for identity in login.users.values() {
        ensure!(
            identities.values().any(|a| a == identity),
            "login identity needs a configured API actor"
        );
    }
    let app = Router::new()
        .route("/", get(|| async { Html(page()) }))
        .route("/guide", get(|| async { Html(include_str!("guide.html")) }))
        .route("/auth/login", post(login_user))
        .route("/auth/poll", post(poll_login))
        .route("/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/workspaces", get(list))
        .route("/api/workspaces/{workspace}", get(read))
        .route("/api/workspaces/{workspace}/commands", post(command))
        .with_state(App {
            pool,
            identities: Arc::new(identities),
            login,
        });
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("joppa: http://{host}:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
pub fn snapshot(data: &Value) -> String {
    let escaped = data
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    page().replace("/* SNAPSHOT */", &format!("window.SNAPSHOT={escaped};"))
}

fn page() -> String {
    include_str!("board.html")
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BOARD_SCRIPT */", include_str!("board.js"))
}
