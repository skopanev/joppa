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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request_id: String,
    command: Command,
}
async fn authenticate(
    app: &App,
    headers: &HeaderMap,
) -> std::result::Result<String, (StatusCode, Json<Value>)> {
    let actor = if headers.contains_key(header::AUTHORIZATION) {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "));
        match token {
            Some(t) => app.identities.get(t).cloned(),
            None => None,
        }
    } else if let Some(token) = web_auth::cookie(headers, "joppa_session") {
        sessions::actor(&app.pool, token).await.map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Could not check your session. Please retry."})),
            )
        })?
    } else {
        None
    };
    actor.ok_or((
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":"Please sign in to continue"})),
    ))
}
fn result(value: Result<Value>) -> ApiResult {
    value.map(Json).map_err(|e| {
        (
            if e.is::<store::AccessDenied>() {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::CONFLICT
            },
            Json(json!({"error":format!("{e:#}")})),
        )
    })
}
async fn health(State(app): State<App>) -> (StatusCode, Json<Value>) {
    let check = async {
        let conn = app.pool.get().await?;
        let row = conn.query_one(
            "SELECT to_regclass('joppa.events') IS NOT NULL AND to_regclass('joppa.workspaces') IS NOT NULL", &[]
        ).await?;
        Ok::<bool, anyhow::Error>(row.get(0))
    };
    let healthy = matches!(
        tokio::time::timeout(std::time::Duration::from_secs(3), check).await,
        Ok(Ok(true))
    );
    (
        if healthy {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({"ok":healthy})),
    )
}
async fn list(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list(&app.pool, &actor).await)
}
async fn read(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::read(&app.pool, &workspace, &actor).await)
}
async fn command(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> ApiResult {
    // Browser cookies only grant read access. Writes require integration credentials.
    if !headers.contains_key(header::AUTHORIZATION) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error":"The portal is read-only. Use Joppa MCP to make changes."})),
        ));
    }
    let actor = authenticate(&app, &headers).await?;
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderRequest {
    request_id: String,
    kind: String,
    parent: Option<String>,
    expected: Vec<String>,
    ids: Vec<String>,
}
async fn reorder(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    Json(request): Json<OrderRequest>,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    // The only browser write: a typed ordering operation, protected against CSRF.
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let command = Command::ReorderTree {
        kind: request.kind,
        parent: request.parent,
        expected: request.expected,
        ids: request.ids,
    };
    result(store::execute(&app.pool, &workspace, &actor, &request.request_id, &command).await)
}
async fn me(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        async {
            let conn = app.pool.get().await?;
            let email: Option<String> = conn
                .query_opt(
                    "SELECT email FROM joppa.google_identities WHERE actor=$1",
                    &[&actor],
                )
                .await?
                .map(|row| row.get(0));
            Ok(json!({"actor":actor,"email":email}))
        }
        .await,
    )
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
        web_auth::set_cookie(&headers, "joppa_login", &id, 300)
            .replace("SameSite=Strict", "SameSite=Lax")
            .parse()
            .unwrap(),
    );
    Ok((response, Json(data)))
}
#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
async fn google_callback(
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
async fn poll_login(State(app): State<App>, headers: HeaderMap) -> AuthResult {
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let id = web_auth::cookie(&headers, "joppa_login")
        .ok_or_else(|| auth_error("Start sign-in first"))?;
    let mut response = auth_headers();
    match app.login.poll(id).await.map_err(auth_error)? {
        None => Ok((response, Json(json!({"status":"pending"})))),
        Some((token, actor)) => {
            sessions::create(&app.pool, &token, &actor)
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
async fn logout(State(app): State<App>, headers: HeaderMap) -> AuthResult {
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
    let oauth_origin = login.public_origin()?;
    let embedder = joppa::embeddings::Embedder::from_env()?;
    if let Some(embedder) = embedder.clone() {
        tokio::spawn(joppa::store::search::worker(pool.clone(), embedder));
    }
    let app = Router::new()
        .merge(oauth::routes())
        .merge(mcp::routes())
        .route("/", get(|| async { Html(page()) }))
        .route(
            "/favicon.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("favicon.svg"),
                )
            }),
        )
        .route("/guide", get(|| async { Html(include_str!("guide.html")) }))
        .route("/health", get(health))
        .route("/auth/login", post(login_user))
        .route("/auth/poll", post(poll_login))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/workspaces", get(list))
        .route("/api/workspaces/{workspace}", get(read))
        .route("/api/workspaces/{workspace}/commands", post(command))
        .route("/api/workspaces/{workspace}/order", post(reorder))
        .with_state(App {
            pool,
            identities: Arc::new(identities),
            login,
            oauth_origin,
            embedder,
        })
        .layer(axum::middleware::from_fn(no_store));
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
    page()
        .replace("/* SNAPSHOT */", &format!("window.SNAPSHOT={escaped};"))
        .replace(
            "/favicon.svg",
            &format!(
                "data:image/svg+xml;base64,{}",
                base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    include_str!("favicon.svg")
                )
            ),
        )
}

fn page() -> String {
    include_str!("board.html")
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BOARD_SCRIPT */", include_str!("board.js"))
}
