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
/// Приём файла: сырые байты, имя и тип — в заголовках.
///
/// Ответ несёт отпечаток и размер; само вложение появляется в журнале
/// отдельной командой Attach. Разделено намеренно: загрузка может не дойти
/// до записи (закрыли вкладку), и тогда в базе остаётся неприкаянный файл,
/// а не запись о вложении, которое нельзя открыть.
async fn upload(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult {
    require_write_credential(&headers)?;
    let actor = authenticate_write(&app, &headers).await?;
    // Право проверяется ДО записи файла: иначе чужой смог бы наполнять
    // хранилище, ничего не имея в воркспейсе.
    let _ = result(
        store::read(
            &app.pool,
            &workspace,
            &actor,
            &store::Scope::from_args(&json!({})).unwrap(),
        )
        .await,
    )?;
    result(
        store::put_blob(&app.pool, &body)
            .await
            .map(|(hash, size)| json!({"hash": hash, "size": size})),
    )
}
/// Процентное кодирование для имени файла в заголовке.
///
/// Своё, а не библиотечное: единственное место во всём сервере, где оно
/// нужно, а имя файла приходит от человека и может нести что угодно —
/// перевод строки в заголовке разрезал бы ответ надвое.
fn percent(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}
async fn download(
    State(app): State<App>,
    Path((workspace, hash)): Path<(String, String)>,
    headers: HeaderMap,
) -> std::result::Result<axum::response::Response, (StatusCode, Json<Value>)> {
    use axum::response::IntoResponse;
    let actor = authenticate(&app, &headers).await?;
    match store::get_blob(&app.pool, &workspace, &actor, &hash).await {
        Ok((name, content_type, bytes)) => Ok((
            [
                (header::CONTENT_TYPE, content_type),
                // inline, а не attachment: картинку и текст хочется увидеть,
                // не скачивая. Имя всё равно уходит — на случай сохранения.
                (
                    header::CONTENT_DISPOSITION,
                    format!("inline; filename*=UTF-8''{}", percent(&name)),
                ),
            ],
            bytes,
        )
            .into_response()),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"no such attachment here"})),
        )),
    }
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
        sessions::actor(&app.pool, token)
            .await
            .map_err(|_| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"Could not check your session. Please retry."})),
                )
            })?
            .map(|actor| (actor, store::Access::Write))
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
            } else if e.is::<joppa::commands::ClientField>()
                || e.is::<joppa::objects::RequiredField>()
            {
                StatusCode::UNPROCESSABLE_ENTITY
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
    let scope = store::Scope {
        full: false,
        req: None,
        revision: None,
        history: false,
        target: None,
    };
    result(store::read(&app.pool, &workspace, &actor, &scope).await)
}
#[derive(Deserialize)]
struct BoardQuery {
    level: Option<String>,
    id: Option<String>,
    revision: Option<u32>,
}
async fn board(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(query): Query<BoardQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        store::board_read(
            &app.pool,
            &workspace,
            &actor,
            query.level.as_deref(),
            query.id.as_deref(),
            query.revision,
        )
        .await,
    )
}
#[derive(Deserialize)]
struct ObjectQuery {
    address: Option<String>,
    parent: Option<String>,
    kind: Option<String>,
    revision: Option<u32>,
    history: Option<bool>,
    limit: Option<usize>,
    cursor: Option<String>,
    note_status: Option<String>,
    related_to: Option<String>,
    work_filter: Option<String>,
}
async fn objects(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<ObjectQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        store::read_objects(
            &app.pool,
            &workspace,
            &actor,
            &store::ObjectScope {
                address: q.address.as_deref(),
                parent: q.parent.as_deref(),
                kind: q.kind.as_deref(),
                revision: q.revision,
                history: q.history.unwrap_or(false),
                limit: q.limit.unwrap_or(50),
                cursor: q.cursor.as_deref(),
                note_status: q.note_status.as_deref(),
                related_to: q.related_to.as_deref(),
                work_filter: q.work_filter.as_deref(),
            },
        )
        .await,
    )
}
#[derive(Deserialize)]
struct TaskQuery {
    view: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
    project: Option<String>,
    purpose: Option<String>,
    priorities: Option<String>,
}
async fn tasks(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(query): Query<TaskQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    let priorities = query
        .priorities
        .as_deref()
        .map(|text| text.split(',').map(str::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();
    result(
        store::task_queue_filtered(
            &app.pool,
            &workspace,
            &actor,
            query.view.as_deref().unwrap_or("available"),
            query.limit.unwrap_or(50),
            query.cursor.as_deref(),
            query.project.as_deref(),
            query.purpose.as_deref(),
            &priorities,
        )
        .await,
    )
}
async fn tree_schema(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<ObjectQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::tree_schema(&app.pool, &workspace, &actor, q.revision).await)
}
/// A test run uploads its JUnit report. Only a token may upload, never a browser session.
async fn upload_report(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult {
    // The token is checked before the body is read. The hook prints these texts in git push.
    let unauthorized = |text: &str| (StatusCode::UNAUTHORIZED, Json(json!({ "error": text })));
    if !headers.contains_key(header::AUTHORIZATION) {
        return Err(unauthorized("token required"));
    }
    let (actor, access) = authenticate_grant(&app, &headers)
        .await
        .map_err(|_| unauthorized("token is not valid"))?;
    require_write_credential(&headers)?;
    refuse_read_grant(access)?;
    let upload: store::ReportUpload = serde_json::from_slice(&body).map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("bad report upload: {e}")})),
        )
    })?;
    result(store::upload_report(&app.pool, &workspace, &actor, upload).await)
}
#[derive(Deserialize)]
struct TestsQuery {
    /// One criterion address, or several separated by commas. Without it: the overview.
    ac: Option<String>,
    /// A task address: done while its criterion is green.
    task: Option<String>,
}
async fn criterion_tests(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<TestsQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    if let Some(task) = q.task {
        return result(store::task_status(&app.pool, &workspace, &actor, &task).await);
    }
    let Some(ac) = q.ac else {
        return result(store::tests_overview(&app.pool, &workspace, &actor).await);
    };
    let targets: Vec<&str> = ac.split(',').filter(|a| !a.is_empty()).take(100).collect();
    if targets.len() == 1 {
        return result(store::criterion_tests(&app.pool, &workspace, &actor, targets[0]).await);
    }
    let mut items = vec![];
    for target in targets {
        match store::criterion_tests(&app.pool, &workspace, &actor, target).await {
            Ok(status) => items.push(status),
            Err(e) => return result(Err(e)),
        }
    }
    Ok(Json(json!({"items": items})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentTokenRequest {
    agent: String,
    /// write (the default) or read.
    access: Option<String>,
}
/// The workspace owner creates an agent token; the raw token is in this reply only.
async fn create_agent_token(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AgentTokenRequest>,
) -> ApiResult {
    let actor = authenticate_write(&app, &headers).await?;
    result(
        store::create_agent_token(
            &app.pool,
            &workspace,
            &actor,
            &body.agent,
            body.access.as_deref(),
        )
        .await,
    )
}
async fn revoke_agent_token(
    State(app): State<App>,
    Path((workspace, id)): Path<(String, i64)>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate_write(&app, &headers).await?;
    result(store::revoke_agent_token(&app.pool, &workspace, &actor, id).await)
}
async fn list_agent_tokens(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list_agent_tokens(&app.pool, &workspace, &actor).await)
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
async fn command(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> ApiResult {
    require_write_credential(&headers)?;
    let actor = authenticate_write(&app, &headers).await?;
    if let Err(error) = joppa::commands::reject_actor_field(&request)
        .and_then(|()| joppa::commands::reject_actor_field(&request["command"]))
    {
        return result(Err(error));
    }
    let request: Request = serde_json::from_value(request).map_err(|error| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":error.to_string()})),
        )
    })?;
    result(
        store::execute_remote_input(
            &app.pool,
            &workspace,
            &actor,
            &request.request_id,
            &serde_json::to_value(&request.command).expect("validated command serializes"),
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
    require_write_credential(&headers)?;
    let actor = authenticate_write(&app, &headers).await?;
    // Legacy integration endpoint; browser sessions cannot reorder the tree.
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
            let identity = conn
                .query_opt(
                    "SELECT email,display_name FROM joppa.google_identities WHERE actor=$1",
                    &[&actor],
                )
                .await?;
            let email: Option<String> = identity.as_ref().map(|row| row.get(0));
            let name: Option<String> = identity.as_ref().and_then(|row| row.get(1));
            Ok(json!({"actor":actor,"email":email,"name":name}))
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
    let state = App {
        pool,
        identities: Arc::new(identities),
        login,
        oauth_origin,
        embedder,
    };
    let app = Router::new()
        .merge(oauth::routes())
        .merge(mcp::routes())
        .route("/", get(public_page))
        .route(
            "/social-preview.png",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/png")],
                    include_bytes!("social-preview.png").as_slice(),
                )
            }),
        )
        .route(
            "/favicon.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("favicon.svg"),
                )
            }),
        )
        .route("/tree", get(|| async { Html(tree_page()) }))
        .route(
            "/guide",
            get(|| async {
                Html(
                    include_str!("guide.html")
                        .replace("/* BRAND_STYLE */", include_str!("brand.css")),
                )
            }),
        )
        .route("/health", get(health))
        .route("/auth/login", post(login_user))
        .route("/auth/poll", post(poll_login))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/workspaces", get(list))
        .route("/api/workspaces/{workspace}", get(read))
        .route("/api/workspaces/{workspace}/board", get(board))
        .route("/api/workspaces/{workspace}/objects", get(objects))
        .route("/api/workspaces/{workspace}/tasks", get(tasks))
        .route("/api/workspaces/{workspace}/tree", get(tree_schema))
        .route(
            "/api/workspaces/{workspace}/reports",
            post(upload_report).layer(axum::extract::DefaultBodyLimit::max(
                store::REPORT_MAX + 64 * 1024,
            )),
        )
        .route("/api/workspaces/{workspace}/tests", get(criterion_tests))
        .route(
            "/api/workspaces/{workspace}/agent-tokens",
            get(list_agent_tokens).post(create_agent_token),
        )
        .route(
            "/api/workspaces/{workspace}/agent-tokens/{id}/revoke",
            post(revoke_agent_token),
        )
        .route("/api/workspaces/{workspace}/commands", post(command))
        .route("/api/workspaces/{workspace}/order", post(reorder))
        // Умолчание axum — два мегабайта, и файл крупнее отлетал бы с
        // невнятным 413 раньше, чем дойдёт до нашей проверки предела.
        .route(
            "/api/workspaces/{workspace}/blobs",
            post(upload).layer(axum::extract::DefaultBodyLimit::max(store::ATTACHMENT_MAX)),
        )
        .route("/api/workspaces/{workspace}/blobs/{hash}", get(download))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            refuse_read_writes,
        ))
        .layer(axum::middleware::from_fn(no_store));
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("joppa: http://{host}:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
pub fn snapshot(data: &Value) -> anyhow::Result<String> {
    let mut data = data.clone();
    let state = serde_json::from_value::<joppa::model::Workspace>(data["state"].clone())?;
    {
        data["snapshot_schema"] = serde_json::json!(joppa::objects::schema(&state));
        let mut objects = serde_json::Map::new();
        for (key, object) in &state.objects {
            objects.insert(key.clone(), serde_json::json!({
                "address":key,"kind":object.kind,"id":object.id,
                "current_revision":object.current().revision,"revision_count":object.current().revision,
                "revisions":object.revisions,"signal":joppa::objects::signal(&state,key),
                "process":joppa::objects::process(&state,key),
                "work_ready":joppa::objects::work_ready(&state,key),
                "review_required":!joppa::objects::review_links(&state,key).is_empty(),
                "annotations":state.notes.get(&joppa::objects::annotation_address(&state,key)),
                "item_id":data["system_ids"][&object.kind][&object.id],
            }));
        }
        data["snapshot_objects"] = serde_json::json!(objects);
    }
    let escaped = data
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    Ok(page()
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
        ))
}

fn page() -> String {
    page_at("https://joppa.otion.us")
}

async fn public_page(State(app): State<App>) -> Html<String> {
    Html(page_at(
        app.oauth_origin
            .as_deref()
            .unwrap_or("https://joppa.otion.us"),
    ))
}

fn page_at(origin: &str) -> String {
    let escaped = origin
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    include_str!("board.html")
        .replace("/* PUBLIC_ORIGIN */", &escaped)
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BRAND_STYLE */", include_str!("brand.css"))
        .replace("/* BOARD_SCRIPT */", include_str!("board.js"))
}

fn tree_page() -> String {
    include_str!("tree.html")
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BRAND_STYLE */", include_str!("brand.css"))
        .replace("/* TREE_STYLE */", include_str!("tree.css"))
        .replace("/* TREE_SCRIPT */", include_str!("tree.js"))
}
