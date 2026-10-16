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
            store::enroll_granted(&app.pool, actor, configured, hd.as_deref())
                .await
                .map_err(failure)?;
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
    (
        if e.is::<store::AccessDenied>() {
            StatusCode::NOT_FOUND
        } else if e.is::<joppa::commands::ClientField>() || e.is::<joppa::objects::RequiredField>()
        {
            StatusCode::UNPROCESSABLE_ENTITY
        } else {
            StatusCode::CONFLICT
        },
        Json(json!({"error":format!("{e:#}")})),
    )
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewWorkspace {
    title: String,
}
/// A signed-in browser user creates a workspace and becomes its owner and only member.
/// The server chooses both IDs, so a repeated request creates another workspace.
/// Commands, batches and MCP keep the CLI-only creation refusal.
async fn create_workspace(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<NewWorkspace>,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Sign in with the browser to create a workspace"));
    }
    let actor = authenticate_write(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let workspace = uuid::Uuid::new_v4().to_string();
    let command = Command::Workspace {
        title: body.title.clone(),
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    result(
        store::execute(&app.pool, &workspace, &actor, &request_id, &command)
            .await
            .map(|_| json!({"id": workspace, "title": body.title})),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailGrant {
    workspace: String,
    email: String,
}
/// The owner grants a workspace to a Google email from a signed-in browser.
async fn grant_email(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<EmailGrant>,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Sign in with the browser to grant access"));
    }
    let actor = authenticate_write(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    result(store::grant_email(&app.pool, &body.workspace, &actor, &body.email).await)
}
async fn list_email_grants(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list_email_grants(&app.pool, &actor).await)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainGrant {
    workspace: String,
    domain: String,
}
/// The owner grants a workspace to a whole email domain from a signed-in browser.
async fn grant_domain(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<DomainGrant>,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Sign in with the browser to grant access"));
    }
    let actor = authenticate_write(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    result(store::grant_domain(&app.pool, &body.workspace, &actor, &body.domain).await)
}
async fn list_domain_grants(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list_domain_grants(&app.pool, &actor).await)
}
async fn settings_page() -> Html<String> {
    Html(
        include_str!("settings.html")
            .replace("/* BOARD_STYLE */", include_str!("board.css"))
            .replace("/* BRAND_STYLE */", include_str!("brand.css")),
    )
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
    let targets: Vec<&str> = ac.split(',').filter(|a| !a.is_empty()).collect();
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
    check_session_origin(&headers)?;
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
    check_session_origin(&headers)?;
    result(store::revoke_agent_token(&app.pool, &workspace, &actor, id).await)
}
/// Cookie sessions pass the shared origin check; bearer calls carry no ambient credential.
fn check_session_origin(headers: &HeaderMap) -> std::result::Result<(), (StatusCode, Json<Value>)> {
    if headers.contains_key(header::AUTHORIZATION) {
        return Ok(());
    }
    web_auth::check_origin(headers).map_err(auth_error)
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
            &request.command,
        )
        .await,
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyRequest {
    request_id: String,
    source: String,
    target: String,
    action: String,
}
/// A signed-in member adds or removes one blocking dependency from the board. Only a
/// browser session from this origin with a write grant may; commands and reorder stay
/// closed to cookie sessions.
async fn dependency(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    Json(request): Json<DependencyRequest>,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error(
            "Sign in with the browser to change a dependency",
        ));
    }
    let actor = authenticate_write(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let add = match request.action.as_str() {
        "add" => true,
        "remove" => false,
        _ => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error":"action must be add or remove"})),
            ));
        }
    };
    result(
        store::save_dependency(
            &app.pool,
            &workspace,
            &actor,
            &request.request_id,
            &request.source,
            &request.target,
            add,
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
        Some((token, actor, hd)) => {
            sessions::create(&app.pool, &token, &actor, hd.as_deref())
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
        .route(
            "/vendor/markdown.js",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                        (header::CACHE_CONTROL, "public, max-age=86400"),
                    ],
                    concat!(
                        include_str!("vendor/marked.umd.js"),
                        "\n",
                        include_str!("vendor/purify.min.js")
                    ),
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
        .route("/settings", get(settings_page))
        .route("/api/workspaces", get(list).post(create_workspace))
        .route(
            "/api/email-grants",
            get(list_email_grants).post(grant_email),
        )
        .route(
            "/api/domain-grants",
            get(list_domain_grants).post(grant_domain),
        )
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
        .route("/api/workspaces/{workspace}/dependencies", post(dependency))
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

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    fn browser(cookie: Option<&str>, origin: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "joppa.test".parse().unwrap());
        headers.insert(header::ORIGIN, origin.parse().unwrap());
        if let Some(cookie) = cookie {
            headers.insert(header::COOKIE, cookie.parse().unwrap());
        }
        headers
    }

    /// A verified Google identity completes sign-in through the callback outcome and the poll.
    async fn sign_in(app: &App, sub: &str, email: &str) -> AuthResult {
        sign_in_hd(app, sub, email, None).await
    }

    /// The same sign-in, with the Google hd claim when Some.
    async fn sign_in_hd(app: &App, sub: &str, email: &str, hd: Option<&str>) -> AuthResult {
        let identity = serde_json::from_value::<crate::google_auth::Identity>(
            json!({"sub":sub,"email":email,"email_verified":true,"hd":hd,"nonce":"n"}),
        )
        .unwrap();
        let (id, _) = app.login.start(None).await.unwrap();
        app.login
            .verified_for_tests(&app.pool, &id, &identity)
            .await;
        let cookie = format!("joppa_login={id}");
        poll_login(
            State(app.clone()),
            browser(Some(&cookie), "http://joppa.test"),
        )
        .await
    }

    fn session(headers: &HeaderMap) -> String {
        headers
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| v.to_str().ok()?.strip_prefix("joppa_session="))
            .and_then(|v| v.split(';').next())
            .map(|token| format!("joppa_session={token}"))
            .unwrap()
    }

    async fn count(app: &App, sql: &str, actor: &str) -> i64 {
        app.pool
            .get()
            .await
            .unwrap()
            .query_one(sql, &[&actor])
            .await
            .unwrap()
            .get(0)
    }

    const ORIGIN: &str = "http://joppa.test";

    /// The app on the isolated test database, with one invited user and no rules file.
    async fn test_app() -> Result<App> {
        let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
        let db: String = pool
            .get()
            .await?
            .query_one("SELECT current_database()", &[])
            .await?
            .get(0);
        ensure!(db.starts_with("joppa_test"), "test database required");
        store::migrate(&pool).await?;
        Ok(App {
            pool,
            identities: Default::default(),
            login: web_auth::Login::for_tests(),
            oauth_origin: None,
            embedder: None,
        })
    }

    async fn create(pool: &Pool, workspace: &str, owner: &str) -> Result<()> {
        let command = Command::Workspace {
            title: workspace.into(),
        };
        store::execute(pool, workspace, owner, "create", &command).await?;
        Ok(())
    }

    /// A browser session made without a sign-in.
    async fn session_for(pool: &Pool, actor: &str) -> Result<String> {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        sessions::create(pool, &token, actor, None).await?;
        Ok(format!("joppa_session={token}"))
    }

    async fn scalar(
        app: &App,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> i64 {
        let conn = app.pool.get().await.unwrap();
        conn.query_one(sql, params).await.unwrap().get(0)
    }

    /// The member list of W as its owner reads it, and every email and domain grant of W.
    async fn members_and_grants(app: &App, w: &str, owner: &str) -> (Value, Vec<String>) {
        let Json(index) = read(
            State(app.clone()),
            Path(w.into()),
            browser(Some(owner), ORIGIN),
        )
        .await
        .unwrap();
        let grants = app
            .pool
            .get()
            .await
            .unwrap()
            .query(
                "SELECT email FROM joppa.email_grants WHERE workspace=$1
                 UNION ALL SELECT 'domain '||domain FROM joppa.domain_grants WHERE workspace=$1 ORDER BY 1",
                &[&w],
            )
            .await
            .unwrap()
            .iter()
            .map(|row| row.get(0))
            .collect();
        (index["index"]["members"].clone(), grants)
    }

    /// The workspace IDs that GET /api/workspaces lists for these headers.
    async fn listed(app: &App, headers: HeaderMap) -> Vec<String> {
        let Json(items) = list(State(app.clone()), headers).await.unwrap();
        let ids = items.as_array().unwrap().iter();
        ids.map(|item| item["id"].as_str().unwrap().to_owned())
            .collect()
    }

    fn plain(reply: ApiResult) -> std::result::Result<Value, (StatusCode, Value)> {
        reply
            .map(|Json(value)| value)
            .map_err(|(status, Json(value))| (status, value))
    }

    async fn add_email(app: &App, cookie: &str, workspace: &str, email: &str) -> ApiResult {
        let body = EmailGrant {
            workspace: workspace.into(),
            email: email.into(),
        };
        grant_email(
            State(app.clone()),
            browser(Some(cookie), ORIGIN),
            Json(body),
        )
        .await
    }

    async fn add_domain(app: &App, cookie: &str, workspace: &str, domain: &str) -> ApiResult {
        let body = DomainGrant {
            workspace: workspace.into(),
            domain: domain.into(),
        };
        grant_domain(
            State(app.clone()),
            browser(Some(cookie), ORIGIN),
            Json(body),
        )
        .await
    }

    fn derived(sub: &str) -> String {
        format!("user-{:x}", sha2::Sha256::digest(sub.as_bytes()))
    }

    const MEMBER_OF: &str = "SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2";
    /// The Google hd of a company account in healthium.one.
    const HEALTHIUM: Option<&str> = Some("healthium.one");
    const SESSIONS_OF: &str = "SELECT count(*) FROM joppa.browser_sessions WHERE actor=$1";

    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn owner_grants_an_email_and_the_google_account_reads_only_that_workspace() -> Result<()>
    {
        let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
        let db: String = pool
            .get()
            .await?
            .query_one("SELECT current_database()", &[])
            .await?
            .get(0);
        ensure!(db.starts_with("joppa_test"), "test database required");
        store::migrate(&pool).await?;
        let app = App {
            pool: pool.clone(),
            identities: Default::default(),
            login: web_auth::Login::for_tests(),
            oauth_origin: None,
            embedder: None,
        };
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (sam, other_owner) = (format!("sam-{id}"), format!("olga-{id}"));
        let (w, other) = (format!("granted-{id}"), format!("other-{id}"));
        for (workspace, owner) in [(&w, &sam), (&other, &other_owner)] {
            store::execute(
                &pool,
                workspace,
                owner,
                "create",
                &Command::Workspace {
                    title: workspace.clone(),
                },
            )
            .await?;
        }
        let (sam_cookie, olga_cookie) = (
            format!("{id}{}", uuid::Uuid::new_v4().simple()),
            format!("{}{id}", uuid::Uuid::new_v4().simple()),
        );
        sessions::create(&pool, &sam_cookie, &sam, None).await?;
        sessions::create(&pool, &olga_cookie, &other_owner, None).await?;
        let (sam_cookie, olga_cookie) = (
            format!("joppa_session={sam_cookie}"),
            format!("joppa_session={olga_cookie}"),
        );
        let origin = "http://joppa.test";
        let grant = |headers: HeaderMap, workspace: &str| {
            grant_email(
                State(app.clone()),
                headers,
                Json(EmailGrant {
                    workspace: workspace.into(),
                    email: "Maria@Example.com".into(),
                }),
            )
        };
        let grants = "SELECT count(*) FROM joppa.email_grants WHERE email=$1";
        // Non-owners, tokens and cross-origin requests record no grant.
        let refused = grant(browser(Some(&olga_cookie), origin), &w)
            .await
            .unwrap_err();
        assert_eq!(refused.1.0["error"], "workspace owner required");
        let mut token = browser(Some(&sam_cookie), origin);
        token.insert(header::AUTHORIZATION, "Bearer x".parse().unwrap());
        assert_eq!(grant(token, &w).await.unwrap_err().0, StatusCode::FORBIDDEN);
        let cross = grant(browser(Some(&sam_cookie), "https://evil.test"), &w).await;
        assert_eq!(cross.unwrap_err().0, StatusCode::FORBIDDEN);
        let anonymous = grant(browser(None, origin), &w).await;
        assert_eq!(anonymous.unwrap_err().0, StatusCode::UNAUTHORIZED);
        assert_eq!(count(&app, grants, "maria@example.com").await, 0);
        // A verified email with no grant, rule or user entry cannot sign in.
        let refused = sign_in(&app, "google-maria", "maria@example.com").await;
        assert_eq!(refused.unwrap_err().0, StatusCode::FORBIDDEN);
        let derived = format!("user-{:x}", sha2::Sha256::digest(b"google-maria"));
        let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
        let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
        let sessions_of = "SELECT count(*) FROM joppa.browser_sessions WHERE actor=$1";
        for sql in [members, bound, sessions_of] {
            assert_eq!(count(&app, sql, &derived).await, 0);
        }
        // The owner grants W twice; one lowercase grant remains.
        for _ in 0..2 {
            let Json(granted) = grant(browser(Some(&sam_cookie), origin), &w).await.unwrap();
            assert_eq!(granted, json!({"workspace":w,"email":"maria@example.com"}));
        }
        assert_eq!(count(&app, grants, "maria@example.com").await, 1);
        let Json(listed) =
            list_email_grants(State(app.clone()), browser(Some(&sam_cookie), origin))
                .await
                .unwrap();
        assert_eq!(
            listed,
            json!({"items":[{"id":w,"title":w,"emails":["maria@example.com"]}]})
        );
        // Maria signs in twice; the second sign-in adds no membership.
        let (headers, Json(ready)) = sign_in(&app, "google-maria", "maria@example.com")
            .await
            .unwrap();
        assert_eq!(ready["actor"], derived.as_str());
        let maria = session(&headers);
        assert_eq!(count(&app, sessions_of, &derived).await, 1);
        let (_, Json(again)) = sign_in(&app, "google-maria", "maria@example.com")
            .await
            .unwrap();
        assert_eq!(again["actor"], derived.as_str());
        assert_eq!(count(&app, members, &derived).await, 1);
        let Json(list_reply) = list(State(app.clone()), browser(Some(&maria), origin))
            .await
            .unwrap();
        assert_eq!(list_reply, json!([{"id":w,"title":w}]));
        let Json(index) = read(
            State(app.clone()),
            Path(w.clone()),
            browser(Some(&maria), origin),
        )
        .await
        .unwrap();
        assert_eq!(index["index"]["owner"], sam.as_str());
        assert_eq!(
            index["index"]["members"],
            json!({sam.clone():"human", derived.clone():"human"})
        );
        let (status, Json(error)) = read(
            State(app.clone()),
            Path(other.clone()),
            browser(Some(&maria), origin),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error":"Workspace not found"}));
        // Another Google account cannot claim the bound email.
        let stolen = sign_in(&app, "google-other", "maria@example.com").await;
        assert_eq!(stolen.unwrap_err().0, StatusCode::FORBIDDEN);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn a_member_who_is_not_the_owner_cannot_add_maria_and_she_has_no_workspace_access()
    -> Result<()> {
        let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
        let db: String = pool
            .get()
            .await?
            .query_one("SELECT current_database()", &[])
            .await?
            .get(0);
        ensure!(db.starts_with("joppa_test"), "test database required");
        store::migrate(&pool).await?;
        let app = App {
            pool: pool.clone(),
            identities: Default::default(),
            login: web_auth::Login::for_tests(),
            oauth_origin: None,
            embedder: None,
        };
        // Maria has no grant, login entry or rule; other tests in this database may grant her.
        pool.get()
            .await?
            .execute(
                "DELETE FROM joppa.email_grants WHERE email='maria@example.com'",
                &[],
            )
            .await?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (sam, alex, w) = (format!("sam-{id}"), format!("alex-{id}"), format!("w-{id}"));
        let create = Command::Workspace { title: w.clone() };
        store::execute(&pool, &w, &sam, "create", &create).await?;
        let member = Command::Member {
            id: alex.clone(),
            role: "human".into(),
        };
        store::execute(&pool, &w, &sam, "add-alex", &member).await?;
        let origin = "http://joppa.test";
        let mut cookies = vec![];
        for actor in [&sam, &alex] {
            let token = format!("{}{}", uuid::Uuid::new_v4().simple(), id);
            sessions::create(&pool, &token, actor, None).await?;
            cookies.push(format!("joppa_session={token}"));
        }
        let (sam_cookie, alex_cookie) = (&cookies[0], &cookies[1]);
        // S and alex read W; the member list and the grants of W are the evidence.
        let snapshot = || async {
            let mut members = vec![];
            for cookie in [sam_cookie, alex_cookie] {
                let Json(index) = read(
                    State(app.clone()),
                    Path(w.clone()),
                    browser(Some(cookie), origin),
                )
                .await
                .unwrap();
                members.push(index["index"]["members"].clone());
            }
            let grants: Vec<String> = app
                .pool
                .get()
                .await
                .unwrap()
                .query(
                    "SELECT email FROM joppa.email_grants WHERE workspace=$1 ORDER BY email",
                    &[&w],
                )
                .await
                .unwrap()
                .iter()
                .map(|row| row.get(0))
                .collect();
            (members, grants)
        };
        let before = snapshot().await;
        assert_eq!(
            before.0,
            vec![json!({sam.clone():"human", alex.clone():"human"}); 2]
        );
        assert!(before.1.is_empty());
        let sub = format!("google-maria-{id}");
        let maria = format!("user-{:x}", sha2::Sha256::digest(sub.as_bytes()));
        let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
        let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
        let sessions_of = "SELECT count(*) FROM joppa.browser_sessions WHERE actor=$1";
        let refused_sign_in = || async {
            let (status, Json(error)) = sign_in(&app, &sub, "maria@example.com").await.unwrap_err();
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert_eq!(error, json!({"error":"No workspace access"}));
            for sql in [members, bound, sessions_of] {
                assert_eq!(count(&app, sql, &maria).await, 0);
            }
        };
        refused_sign_in().await;
        let refused = grant_email(
            State(app.clone()),
            browser(Some(alex_cookie), origin),
            Json(EmailGrant {
                workspace: w.clone(),
                email: "maria@example.com".into(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.1.0, json!({"error":"workspace owner required"}));
        assert_eq!(snapshot().await, before);
        refused_sign_in().await;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn owner_adds_a_blank_or_malformed_email_and_w_does_not_change() -> Result<()> {
        let app = test_app().await?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (sam, w) = (format!("sam-{id}"), format!("w-{id}"));
        create(&app.pool, &w, &sam).await?;
        let sam_cookie = session_for(&app.pool, &sam).await?;
        let before = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(before, (json!({sam.clone():"human"}), vec![]));
        for input in ["  ", "maria-at-example"] {
            let reply = plain(add_email(&app, &sam_cookie, &w, input).await);
            let refused = json!({"error":"a valid email is required"});
            assert_eq!(
                reply,
                Err((StatusCode::UNPROCESSABLE_ENTITY, refused)),
                "input {input:?}"
            );
            let after = members_and_grants(&app, &w, &sam_cookie).await;
            assert_eq!(after, before, "input {input:?} changed W; reply {reply:?}");
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn a_signed_in_person_reads_a_newly_granted_workspace_after_the_next_page_load()
    -> Result<()> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let sub = format!("google-alex-{id}");
        let alex = derived(&sub);
        let alex_token = format!("alex-token-{id}");
        let app = App {
            identities: Arc::new(BTreeMap::from([(alex_token.clone(), alex.clone())])),
            ..test_app().await?
        };
        let pool = &app.pool;
        // This test owns alex@example.com in the shared test database.
        let conn = pool.get().await?;
        conn.execute(
            "DELETE FROM joppa.email_grants WHERE email='alex@example.com'",
            &[],
        )
        .await?;
        conn.execute(
            "DELETE FROM joppa.google_identities WHERE email='alex@example.com'",
            &[],
        )
        .await?;
        drop(conn);
        let (sam, olga) = (format!("sam-{id}"), format!("olga-{id}"));
        let (home, w, control) = (
            format!("home-{id}"),
            format!("w-{id}"),
            format!("control-{id}"),
        );
        create(pool, &home, &olga).await?;
        create(pool, &w, &sam).await?;
        create(pool, &control, &sam).await?;
        let (sam_cookie, olga_cookie) = (
            session_for(pool, &sam).await?,
            session_for(pool, &olga).await?,
        );
        // Alex signs in with Google through a grant on another workspace.
        plain(add_email(&app, &olga_cookie, &home, "alex@example.com").await).unwrap();
        let (headers, Json(ready)) = sign_in(&app, &sub, "alex@example.com").await.unwrap();
        assert_eq!(ready["actor"], alex.as_str());
        let alex_cookie = session(&headers);
        assert_eq!(scalar(&app, MEMBER_OF, &[&w, &alex]).await, 0);
        // S grants W. Token requests for alex and for an agent enroll nobody.
        plain(add_email(&app, &sam_cookie, &w, "alex@example.com").await).unwrap();
        let granted = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(granted.0, json!({sam.clone():"human"}));
        let mut bearer = HeaderMap::new();
        bearer.insert(
            header::AUTHORIZATION,
            format!("Bearer {alex_token}").parse()?,
        );
        assert_eq!(listed(&app, bearer.clone()).await, vec![home.clone()]);
        let refused = plain(read(State(app.clone()), Path(w.clone()), bearer).await);
        assert_eq!(refused.unwrap_err().0, StatusCode::NOT_FOUND);
        let agent =
            store::create_agent_token(pool, &control, &sam, &format!("bot-{id}"), None).await?;
        let mut bearer = HeaderMap::new();
        let token = agent["token"].as_str().unwrap();
        bearer.insert(header::AUTHORIZATION, format!("Bearer {token}").parse()?);
        assert_eq!(listed(&app, bearer).await, vec![control.clone()]);
        assert_eq!(members_and_grants(&app, &w, &sam_cookie).await, granted);
        // Alex reloads twice with the same session; only the first reload enrolls.
        let events = "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND actor='joppa-access' AND request_id=$2";
        let request = format!("enroll-{alex}");
        for reload in 1..=2 {
            let Json(me_reply) = me(State(app.clone()), browser(Some(&alex_cookie), ORIGIN))
                .await
                .unwrap();
            assert_eq!(me_reply["actor"], alex.as_str(), "reload {reload}");
            assert_eq!(me_reply["email"], "alex@example.com", "reload {reload}");
            let mut ids = listed(&app, browser(Some(&alex_cookie), ORIGIN)).await;
            ids.sort();
            let mut expected = vec![home.clone(), w.clone()];
            expected.sort();
            assert_eq!(ids, expected, "reload {reload}");
            let (members, grants) = members_and_grants(&app, &w, &sam_cookie).await;
            assert_eq!(
                members,
                json!({sam.clone():"human", alex.clone():"human"}),
                "reload {reload}"
            );
            assert_eq!(grants, granted.1, "reload {reload}");
            assert_eq!(
                scalar(&app, events, &[&w, &request]).await,
                1,
                "reload {reload}"
            );
            assert_eq!(
                scalar(&app, SESSIONS_OF, &[&alex]).await,
                1,
                "reload {reload}"
            );
        }
        let (status, error) = plain(
            read(
                State(app.clone()),
                Path(control.clone()),
                browser(Some(&alex_cookie), ORIGIN),
            )
            .await,
        )
        .unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error":"Workspace not found"}));
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn a_configured_user_without_a_google_identity_reads_a_newly_granted_workspace()
    -> Result<()> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (casey, email) = (format!("casey-{id}"), format!("casey-{id}@example.test"));
        let mut app = test_app().await?;
        app.login.users.insert(email.clone(), casey.clone());
        let pool = &app.pool;
        let sam = format!("sam-{id}");
        let (earlier, w, control) = (
            format!("earlier-{id}"),
            format!("w-{id}"),
            format!("control-{id}"),
        );
        create(pool, &earlier, &casey).await?;
        create(pool, &w, &sam).await?;
        create(pool, &control, &sam).await?;
        let (sam_cookie, casey_cookie) = (
            session_for(pool, &sam).await?,
            session_for(pool, &casey).await?,
        );
        let casey_reads = || browser(Some(&casey_cookie), ORIGIN);
        assert_eq!(listed(&app, casey_reads()).await, vec![earlier.clone()]);
        plain(add_email(&app, &sam_cookie, &w, &email).await).unwrap();
        // Casey reloads twice with the same session; only the first reload enrolls.
        let events = "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND actor='joppa-access' AND request_id=$2";
        let request = format!("enroll-{casey}");
        for reload in 1..=2 {
            let Json(me_reply) = me(State(app.clone()), casey_reads()).await.unwrap();
            assert_eq!(me_reply["actor"], casey.as_str(), "reload {reload}");
            let mut ids = listed(&app, casey_reads()).await;
            ids.sort();
            let mut expected = vec![earlier.clone(), w.clone()];
            expected.sort();
            assert_eq!(ids, expected, "reload {reload}");
            let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
            assert_eq!(
                members,
                json!({sam.clone():"human", casey.clone():"human"}),
                "reload {reload}"
            );
            assert_eq!(
                scalar(&app, events, &[&w, &request]).await,
                1,
                "reload {reload}"
            );
            assert_eq!(
                scalar(&app, SESSIONS_OF, &[&casey]).await,
                1,
                "reload {reload}"
            );
        }
        let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1 OR email=$2";
        assert_eq!(scalar(&app, bound, &[&casey, &email]).await, 0);
        let (status, error) =
            plain(read(State(app.clone()), Path(control.clone()), casey_reads()).await)
                .unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error":"Workspace not found"}));
        Ok(())
    }

    // owner-adds-members-ac9
    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn owner_adds_a_domain_and_its_google_people_read_w_as_members() -> Result<()> {
        let app = test_app().await?;
        let pool = &app.pool;
        let id = uuid::Uuid::new_v4().simple().to_string();
        // This test owns healthium.one in the shared test database.
        let conn = pool.get().await?;
        conn.execute(
            "DELETE FROM joppa.domain_grants WHERE domain='healthium.one'",
            &[],
        )
        .await?;
        conn.execute(
            "DELETE FROM joppa.email_grants WHERE email IN ('maria@healthium.one','lena@healthium.one')",
            &[],
        )
        .await?;
        drop(conn);
        let (sam, olga) = (format!("sam-{id}"), format!("olga-{id}"));
        let (home, w, control) = (
            format!("home-{id}"),
            format!("w-{id}"),
            format!("control-{id}"),
        );
        create(pool, &home, &olga).await?;
        create(pool, &w, &sam).await?;
        create(pool, &control, &sam).await?;
        let (sam_cookie, olga_cookie) = (
            session_for(pool, &sam).await?,
            session_for(pool, &olga).await?,
        );
        // Maria is signed in through an email grant on another workspace.
        plain(add_email(&app, &olga_cookie, &home, "maria@healthium.one").await).unwrap();
        let maria_sub = format!("google-maria-{id}");
        let maria = derived(&maria_sub);
        let (headers, _) = sign_in_hd(&app, &maria_sub, "maria@healthium.one", HEALTHIUM)
            .await
            .unwrap();
        let maria_cookie = session(&headers);
        assert_eq!(scalar(&app, MEMBER_OF, &[&w, &maria]).await, 0);
        // S adds the domain twice in another case; one lowercase grant remains.
        for _ in 0..2 {
            let reply = plain(add_domain(&app, &sam_cookie, &w, "HEALTHIUM.one").await);
            assert_eq!(reply, Ok(json!({"workspace":w,"domain":"healthium.one"})));
            let state = members_and_grants(&app, &w, &sam_cookie).await;
            assert_eq!(
                state,
                (
                    json!({sam.clone():"human"}),
                    vec!["domain healthium.one".into()]
                )
            );
        }
        let Json(domains) =
            list_domain_grants(State(app.clone()), browser(Some(&sam_cookie), ORIGIN))
                .await
                .unwrap();
        let mut expected = vec![
            json!({"id":control,"title":control,"domains":[]}),
            json!({"id":w,"title":w,"domains":["healthium.one"]}),
        ];
        expected.sort_by_key(|item| item["id"].as_str().unwrap().to_owned());
        assert_eq!(domains, json!({"items": expected}));
        // Maria reads the board again with the same session.
        let Json(me_reply) = me(State(app.clone()), browser(Some(&maria_cookie), ORIGIN))
            .await
            .unwrap();
        assert_eq!(me_reply["actor"], maria.as_str());
        let mut ids = listed(&app, browser(Some(&maria_cookie), ORIGIN)).await;
        ids.sort();
        let mut expected = vec![home.clone(), w.clone()];
        expected.sort();
        assert_eq!(ids, expected);
        assert_eq!(scalar(&app, SESSIONS_OF, &[&maria]).await, 1);
        // Lena signs in with Google for the first time.
        let lena_sub = format!("google-lena-{id}");
        let lena = derived(&lena_sub);
        let (headers, Json(ready)) = sign_in_hd(&app, &lena_sub, "lena@healthium.one", HEALTHIUM)
            .await
            .unwrap();
        assert_eq!(ready["actor"], lena.as_str());
        let lena_cookie = session(&headers);
        assert_eq!(scalar(&app, SESSIONS_OF, &[&lena]).await, 1);
        assert_eq!(
            listed(&app, browser(Some(&lena_cookie), ORIGIN)).await,
            vec![w.clone()]
        );
        let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            members,
            json!({sam.clone():"human", maria.clone():"human", lena.clone():"human"})
        );
        let (status, error) = plain(
            read(
                State(app.clone()),
                Path(control.clone()),
                browser(Some(&lena_cookie), ORIGIN),
            )
            .await,
        )
        .unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error":"Workspace not found"}));
        // A verified email in another domain has no grant or rule.
        let ivan_sub = format!("google-ivan-{id}");
        let ivan = derived(&ivan_sub);
        let (status, Json(error)) = sign_in(&app, &ivan_sub, &format!("ivan@other-{id}.test"))
            .await
            .unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error, json!({"error":"No workspace access"}));
        let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
        let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
        for sql in [members, bound, SESSIONS_OF] {
            assert_eq!(scalar(&app, sql, &[&ivan]).await, 0, "{sql}");
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn a_non_owner_or_an_invalid_domain_leaves_w_unchanged() -> Result<()> {
        let app = test_app().await?;
        let pool = &app.pool;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (sam, alex, w) = (format!("sam-{id}"), format!("alex-{id}"), format!("w-{id}"));
        create(pool, &w, &sam).await?;
        let member = Command::Member {
            id: alex.clone(),
            role: "human".into(),
        };
        store::execute(pool, &w, &sam, "add-alex", &member).await?;
        let (sam_cookie, alex_cookie) = (
            session_for(pool, &sam).await?,
            session_for(pool, &alex).await?,
        );
        let before = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            before,
            (json!({sam.clone():"human", alex.clone():"human"}), vec![])
        );
        let reply = plain(add_domain(&app, &alex_cookie, &w, "healthium.one").await);
        let owner_required = json!({"error":"workspace owner required"});
        assert_eq!(reply, Err((StatusCode::CONFLICT, owner_required)));
        assert_eq!(members_and_grants(&app, &w, &sam_cookie).await, before);
        let invalid = [
            "healthium",
            "",
            "  ",
            "@healthium.one",
            "maria@healthium.one",
            "health ium.one",
            " healthium.one",
        ];
        for input in invalid {
            let reply = plain(add_domain(&app, &sam_cookie, &w, input).await);
            let refused = json!({"error":"a valid email domain is required"});
            assert_eq!(
                reply,
                Err((StatusCode::UNPROCESSABLE_ENTITY, refused)),
                "input {input:?}"
            );
            let after = members_and_grants(&app, &w, &sam_cookie).await;
            assert_eq!(after, before, "input {input:?} changed W");
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn the_owner_adds_an_existing_member_again_and_w_does_not_change() -> Result<()> {
        let app = test_app().await?;
        let pool = &app.pool;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (sam, w) = (format!("sam-{id}"), format!("w-{id}"));
        create(pool, &w, &sam).await?;
        let sam_cookie = session_for(pool, &sam).await?;
        plain(add_email(&app, &sam_cookie, &w, "maria@example.com").await).unwrap();
        // The email binds one Google subject. Reuse another test's binding instead of deleting it.
        let bound = pool
            .get()
            .await?
            .query_opt(
                "SELECT google_sub FROM joppa.google_identities WHERE email='maria@example.com'",
                &[],
            )
            .await?
            .map(|row| row.get::<_, String>(0));
        let sub = bound.clone().unwrap_or(format!("google-maria-{id}"));
        let maria = derived(&sub);
        let (_, Json(ready)) = sign_in(&app, &sub, "maria@example.com").await.unwrap();
        assert_eq!(ready["actor"], maria.as_str());
        let before = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            before,
            (
                json!({sam.clone():"human", maria.clone():"human"}),
                vec!["maria@example.com".to_owned()]
            )
        );
        let reply = plain(add_email(&app, &sam_cookie, &w, "maria@example.com").await);
        assert_eq!(
            reply,
            Ok(json!({"workspace":w,"email":"maria@example.com"}))
        );
        assert_eq!(members_and_grants(&app, &w, &sam_cookie).await, before);
        // Later tests in this database start without this test's grant and binding.
        let conn = pool.get().await?;
        conn.execute("DELETE FROM joppa.email_grants WHERE workspace=$1", &[&w])
            .await?;
        if bound.is_none() {
            conn.execute(
                "DELETE FROM joppa.google_identities WHERE actor=$1",
                &[&maria],
            )
            .await?;
        }
        Ok(())
    }

    /// AC9 tests own these domains and emails in the shared test database.
    /// They start and end without their grants and Google bindings.
    async fn forget_hd_fixtures(app: &App, emails: &[&str]) -> Result<()> {
        let conn = app.pool.get().await?;
        conn.execute(
            "DELETE FROM joppa.domain_grants WHERE domain IN ('healthium.one','gmail.com')",
            &[],
        )
        .await?;
        let emails: Vec<String> = emails.iter().map(|e| e.to_string()).collect();
        conn.execute(
            "DELETE FROM joppa.email_grants WHERE email=ANY($1)",
            &[&emails],
        )
        .await?;
        conn.execute(
            "DELETE FROM joppa.google_identities WHERE email=ANY($1)",
            &[&emails],
        )
        .await?;
        Ok(())
    }

    /// The stored hd of the browser session in this cookie.
    async fn session_hd(app: &App, cookie: &str) -> Option<String> {
        let token = cookie.strip_prefix("joppa_session=").unwrap();
        let hash = format!("{:x}", sha2::Sha256::digest(token.as_bytes()));
        let conn = app.pool.get().await.unwrap();
        conn.query_one(
            "SELECT hd FROM joppa.browser_sessions WHERE token_hash=$1",
            &[&hash],
        )
        .await
        .unwrap()
        .get(0)
    }

    /// A page load with this cookie, then the workspaces that it lists.
    async fn reload(app: &App, cookie: &str, actor: &str) -> Vec<String> {
        let Json(me_reply) = me(State(app.clone()), browser(Some(cookie), ORIGIN))
            .await
            .unwrap();
        assert_eq!(me_reply["actor"], actor);
        let mut ids = listed(app, browser(Some(cookie), ORIGIN)).await;
        ids.sort();
        ids
    }

    // owner-adds-members-ac9
    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn domain_grant_needs_hd_lena_reads_w_and_people_without_that_hd_do_not() -> Result<()> {
        let mut app = test_app().await?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let casey_email = format!("casey-{id}@healthium.one");
        let emails = [
            "lena@healthium.one",
            "ex@healthium.one",
            "pat@gmail.com",
            "max@healthium.one",
        ];
        forget_hd_fixtures(&app, &emails).await?;
        let casey = format!("casey-{id}");
        app.login.users.insert(casey_email.clone(), casey.clone());
        let pool = app.pool.clone();
        let (sam, olga) = (format!("sam-{id}"), format!("olga-{id}"));
        let (home, w, w2) = (format!("home-{id}"), format!("w-{id}"), format!("w2-{id}"));
        create(&pool, &home, &olga).await?;
        create(&pool, &w, &sam).await?;
        create(&pool, &w2, &sam).await?;
        let (sam_cookie, olga_cookie) = (
            session_for(&pool, &sam).await?,
            session_for(&pool, &olga).await?,
        );
        plain(add_domain(&app, &sam_cookie, &w, "healthium.one").await).unwrap();
        plain(add_domain(&app, &sam_cookie, &w2, "gmail.com").await).unwrap();
        // Lena signs in with the company account.
        let lena_sub = format!("google-lena-{id}");
        let lena = derived(&lena_sub);
        let (headers, Json(ready)) = sign_in_hd(&app, &lena_sub, "lena@healthium.one", HEALTHIUM)
            .await
            .unwrap();
        assert_eq!(ready["actor"], lena.as_str());
        let lena_cookie = session(&headers);
        assert_eq!(session_hd(&app, &lena_cookie).await.as_deref(), HEALTHIUM);
        assert_eq!(reload(&app, &lena_cookie, &lena).await, vec![w.clone()]);
        let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(members, json!({sam.clone():"human", lena.clone():"human"}));
        // Without hd, or with another hd, the domain grant admits nobody.
        let people = [
            ("ex@healthium.one", None),
            ("pat@gmail.com", None),
            ("max@healthium.one", Some("other-hd.test")),
        ];
        for (email, hd) in people {
            let sub = format!("google-{email}-{id}");
            let (status, Json(error)) = sign_in_hd(&app, &sub, email, hd).await.unwrap_err();
            assert_eq!(status, StatusCode::FORBIDDEN, "{email}");
            assert_eq!(error, json!({"error":"No workspace access"}), "{email}");
            let members =
                "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
            let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
            for sql in [members, bound, SESSIONS_OF] {
                assert_eq!(
                    scalar(&app, sql, &[&derived(&sub)]).await,
                    0,
                    "{email}: {sql}"
                );
            }
        }
        // An email grant on home lets them sign in. They still do not read W or W2.
        let mut outsiders = vec![];
        for (email, hd) in people {
            plain(add_email(&app, &olga_cookie, &home, email).await).unwrap();
            let actor = derived(&format!("google-{email}-{id}"));
            let sub = format!("google-{email}-{id}");
            let (headers, _) = sign_in_hd(&app, &sub, email, hd).await.unwrap();
            let cookie = session(&headers);
            assert_eq!(
                reload(&app, &cookie, &actor).await,
                vec![home.clone()],
                "{email}"
            );
            for workspace in [&w, &w2] {
                let refused = plain(
                    read(
                        State(app.clone()),
                        Path(workspace.clone()),
                        browser(Some(&cookie), ORIGIN),
                    )
                    .await,
                );
                assert_eq!(refused.unwrap_err().0, StatusCode::NOT_FOUND, "{email}");
            }
            outsiders.push(actor);
        }
        // A configured actor without a bound identity gets no domain grant, also with hd.
        let casey_cookie = format!("joppa_session={}{}", id, uuid::Uuid::new_v4().simple());
        let token = casey_cookie.strip_prefix("joppa_session=").unwrap();
        sessions::create(&pool, token, &casey, HEALTHIUM).await?;
        assert_eq!(
            reload(&app, &casey_cookie, &casey).await,
            Vec::<String>::new()
        );
        let (w_members, _) = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            w_members,
            json!({sam.clone():"human", lena.clone():"human"})
        );
        let (w2_members, _) = members_and_grants(&app, &w2, &sam_cookie).await;
        assert_eq!(w2_members, json!({sam.clone():"human"}));
        for actor in outsiders.iter().chain([&casey]) {
            assert_eq!(scalar(&app, MEMBER_OF, &[&w, actor]).await, 0, "{actor}");
            assert_eq!(scalar(&app, MEMBER_OF, &[&w2, actor]).await, 0, "{actor}");
        }
        forget_hd_fixtures(&app, &emails).await
    }

    // owner-adds-members-ac9
    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn domain_grant_needs_hd_a_reload_and_an_older_session_do_not_enroll_ex() -> Result<()> {
        let app = test_app().await?;
        let pool = &app.pool;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let emails = ["ex@healthium.one"];
        forget_hd_fixtures(&app, &emails).await?;
        let (sam, olga) = (format!("sam-{id}"), format!("olga-{id}"));
        let (home, w) = (format!("home-{id}"), format!("w-{id}"));
        create(pool, &home, &olga).await?;
        create(pool, &w, &sam).await?;
        let (sam_cookie, olga_cookie) = (
            session_for(pool, &sam).await?,
            session_for(pool, &olga).await?,
        );
        // Ex signs in with no hd through an email grant on home.
        plain(add_email(&app, &olga_cookie, &home, "ex@healthium.one").await).unwrap();
        let ex_sub = format!("google-ex-{id}");
        let ex = derived(&ex_sub);
        let (headers, _) = sign_in(&app, &ex_sub, "ex@healthium.one").await.unwrap();
        let ex_cookie = session(&headers);
        assert_eq!(session_hd(&app, &ex_cookie).await, None);
        // A session made before this rule has a NULL hd.
        let older = format!("{}{id}", uuid::Uuid::new_v4().simple());
        pool.get().await?.execute(
            "INSERT INTO joppa.browser_sessions(token_hash,actor,expires_at) VALUES($1,$2,clock_timestamp()+interval '1 hour')",
            &[&format!("{:x}", sha2::Sha256::digest(older.as_bytes())), &ex],
        ).await?;
        let older = format!("joppa_session={older}");
        assert_eq!(session_hd(&app, &older).await, None);
        // S adds healthium.one to W. Ex reloads with each session.
        plain(add_domain(&app, &sam_cookie, &w, "healthium.one").await).unwrap();
        for cookie in [&ex_cookie, &older, &ex_cookie] {
            assert_eq!(reload(&app, cookie, &ex).await, vec![home.clone()]);
            let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
            assert_eq!(members, json!({sam.clone():"human"}));
        }
        assert_eq!(scalar(&app, MEMBER_OF, &[&w, &ex]).await, 0);
        forget_hd_fixtures(&app, &emails).await
    }

    // owner-adds-members-ac9
    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn domain_grant_needs_hd_of_the_session_that_loads_the_page() -> Result<()> {
        let app = test_app().await?;
        let pool = &app.pool;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let emails = ["kim@healthium.one", "kai@healthium.one"];
        forget_hd_fixtures(&app, &emails).await?;
        let olga = format!("olga-{id}");
        let home = format!("home-{id}");
        create(pool, &home, &olga).await?;
        let olga_cookie = session_for(pool, &olga).await?;
        // Kim signs in with the company hd first. Kai signs in with no hd first.
        for (email, hd_first) in [(emails[0], true), (emails[1], false)] {
            // Each order starts without the domain grant of the other order.
            forget_hd_fixtures(&app, &[]).await?;
            let name = email.split('@').next().unwrap();
            let (sam, w) = (format!("sam-{name}-{id}"), format!("w-{name}-{id}"));
            create(pool, &w, &sam).await?;
            let sam_cookie = session_for(pool, &sam).await?;
            plain(add_email(&app, &olga_cookie, &home, email).await).unwrap();
            let sub = format!("google-{email}-{id}");
            let actor = derived(&sub);
            let mut cookies = vec![];
            for hd in if hd_first {
                [HEALTHIUM, None]
            } else {
                [None, HEALTHIUM]
            } {
                let (headers, _) = sign_in_hd(&app, &sub, email, hd).await.unwrap();
                cookies.push(session(&headers));
            }
            if !hd_first {
                cookies.reverse();
            }
            let (with_hd, without_hd) = (&cookies[0], &cookies[1]);
            assert_eq!(
                session_hd(&app, with_hd).await.as_deref(),
                HEALTHIUM,
                "{email}"
            );
            assert_eq!(session_hd(&app, without_hd).await, None, "{email}");
            assert_eq!(scalar(&app, SESSIONS_OF, &[&actor]).await, 2, "{email}");
            plain(add_domain(&app, &sam_cookie, &w, "healthium.one").await).unwrap();
            // The page load with no hd does not enroll the person.
            assert_eq!(
                reload(&app, without_hd, &actor).await,
                vec![home.clone()],
                "{email}"
            );
            assert_eq!(scalar(&app, MEMBER_OF, &[&w, &actor]).await, 0, "{email}");
            // The page load with the company hd enrolls the person in W as human.
            let mut expected = vec![home.clone(), w.clone()];
            expected.sort();
            assert_eq!(reload(&app, with_hd, &actor).await, expected, "{email}");
            let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
            assert_eq!(
                members,
                json!({sam.clone():"human", actor.clone():"human"}),
                "{email}"
            );
            assert_eq!(session_hd(&app, without_hd).await, None, "{email}");
        }
        forget_hd_fixtures(&app, &emails).await
    }

    // owner-adds-members-ac9
    #[tokio::test]
    #[ignore = "requires isolated test database"]
    async fn domain_grant_needs_hd_but_an_email_grant_does_not_and_hd_case_does_not_matter()
    -> Result<()> {
        let app = test_app().await?;
        let pool = &app.pool;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let emails = ["ex@healthium.one", "lena@healthium.one"];
        forget_hd_fixtures(&app, &emails).await?;
        let sam = format!("sam-{id}");
        let (w, w2) = (format!("w-{id}"), format!("w2-{id}"));
        create(pool, &w, &sam).await?;
        create(pool, &w2, &sam).await?;
        let sam_cookie = session_for(pool, &sam).await?;
        // S grants the single email. Ex signs in with no hd and reads W.
        plain(add_email(&app, &sam_cookie, &w, "ex@healthium.one").await).unwrap();
        let ex_sub = format!("google-ex-{id}");
        let ex = derived(&ex_sub);
        let (headers, _) = sign_in(&app, &ex_sub, "ex@healthium.one").await.unwrap();
        let ex_cookie = session(&headers);
        assert_eq!(session_hd(&app, &ex_cookie).await, None);
        assert_eq!(reload(&app, &ex_cookie, &ex).await, vec![w.clone()]);
        let read_w = read(
            State(app.clone()),
            Path(w.clone()),
            browser(Some(&ex_cookie), ORIGIN),
        );
        let Json(index) = read_w.await.unwrap();
        assert_eq!(
            index["index"]["members"],
            json!({sam.clone():"human", ex.clone():"human"})
        );
        // A mixed-case hd matches the healthium.one domain grant.
        plain(add_domain(&app, &sam_cookie, &w2, "healthium.one").await).unwrap();
        let lena_sub = format!("google-lena-{id}");
        let lena = derived(&lena_sub);
        let (headers, Json(ready)) =
            sign_in_hd(&app, &lena_sub, "lena@healthium.one", Some("HealthIum.One"))
                .await
                .unwrap();
        assert_eq!(ready["actor"], lena.as_str());
        let lena_cookie = session(&headers);
        assert_eq!(session_hd(&app, &lena_cookie).await.as_deref(), HEALTHIUM);
        assert_eq!(reload(&app, &lena_cookie, &lena).await, vec![w2.clone()]);
        let (members, _) = members_and_grants(&app, &w2, &sam_cookie).await;
        assert_eq!(members, json!({sam.clone():"human", lena.clone():"human"}));
        forget_hd_fixtures(&app, &emails).await
    }
}
