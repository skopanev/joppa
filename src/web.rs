use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
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
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"valid bearer token required"})),
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
    Ok(Json(json!({"actor":authenticate(&app, &headers)?})))
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
    let app = Router::new()
        .route("/", get(|| async { Html(page()) }))
        .route("/api/me", get(me))
        .route("/api/workspaces", get(list))
        .route("/api/workspaces/{workspace}", get(read))
        .route("/api/workspaces/{workspace}/commands", post(command))
        .with_state(App {
            pool,
            identities: Arc::new(identities),
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
    include_str!("board.html").replace("/* BOARD_SCRIPT */", include_str!("board.js"))
}
