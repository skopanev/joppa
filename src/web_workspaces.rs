//! Workspace and access routes for the Settings page.
use super::*;

pub(super) async fn health(State(app): State<App>) -> (StatusCode, Json<Value>) {
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
pub(super) async fn list(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list(&app.pool, &actor).await)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewWorkspace {
    title: String,
}
/// A signed-in browser user creates a workspace and becomes its owner and only member.
/// The server chooses both IDs, so a repeated request creates another workspace.
/// Commands, batches and MCP keep the CLI-only creation refusal.
pub(super) async fn create_workspace(
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
pub(super) struct EmailGrant {
    pub(super) workspace: String,
    pub(super) email: String,
}
/// The owner grants a workspace to a Google email from a signed-in browser.
pub(super) async fn grant_email(
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
pub(super) async fn list_email_grants(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list_email_grants(&app.pool, &actor).await)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DomainGrant {
    pub(super) workspace: String,
    pub(super) domain: String,
}
/// The owner grants a workspace to a whole email domain from a signed-in browser.
pub(super) async fn grant_domain(
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
pub(super) async fn list_domain_grants(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list_domain_grants(&app.pool, &actor).await)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceRename {
    title: String,
}
/// The owner renames a workspace from a signed-in browser.
pub(super) async fn rename_workspace(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    Json(body): Json<WorkspaceRename>,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Sign in with the browser to rename a workspace"));
    }
    let actor = authenticate_write(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let command = Command::RenameWorkspace(joppa::commands::RenameWorkspaceOp {
        workspace_id: None,
        title: body.title.clone(),
        reason: "Renamed in Settings".into(),
        reference_prefix: None,
    });
    let request_id = uuid::Uuid::new_v4().to_string();
    result(
        store::execute(&app.pool, &workspace, &actor, &request_id, &command)
            .await
            .map(|_| json!({"id": workspace, "title": body.title})),
    )
}
/// The board is read-only; priority changes go through Joppa MCP.
pub(super) async fn set_priority() -> ApiResult {
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({"error":"The portal is read-only. Use Joppa MCP to make changes."})),
    ))
}
/// The owner reads the workspaces that link to this workspace, before a deletion.
pub(super) async fn workspace_references(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::workspace_references(&app.pool, &workspace, &actor).await)
}
/// The owner deletes a workspace from a signed-in browser. No row of it stays.
pub(super) async fn delete_workspace(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Sign in with the browser to delete a workspace"));
    }
    let actor = authenticate_write(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let command = Command::DeleteWorkspace {
        reason: "Deleted in Settings".into(),
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    result(
        store::execute(&app.pool, &workspace, &actor, &request_id, &command)
            .await
            .map(|r| json!({"id": workspace, "deleted": true, "removed_references": r["removed_references"]})),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AccessRemoval {
    workspace: String,
    #[serde(default)]
    member: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    domain: Option<String>,
}
/// The owner removes a member, an email grant or a domain grant from a signed-in browser.
pub(super) async fn remove_access(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<AccessRemoval>,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Sign in with the browser to remove access"));
    }
    let actor = authenticate_write(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let ws = &body.workspace;
    result(match (&body.member, &body.email, &body.domain) {
        (Some(id), None, None) => store::remove_member(&app.pool, ws, &actor, id).await,
        (None, Some(email), None) => store::revoke_email(&app.pool, ws, &actor, email).await,
        (None, None, Some(domain)) => store::revoke_domain(&app.pool, ws, &actor, domain).await,
        _ => Err(joppa::commands::ClientField("name one member, email or domain").into()),
    })
}
/// The settings page reads its owned workspaces, their people and their grants in one request.
pub(super) async fn list_owned_access(State(app): State<App>, headers: HeaderMap) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list_owned_access(&app.pool, &actor).await)
}
pub(super) async fn settings_page() -> Html<String> {
    Html(
        concat!(
            include_str!("settings.html"),
            include_str!("settings_more.html")
        )
        .replace(
            "/* BOARD_STYLE */",
            concat!(include_str!("board.css"), include_str!("board_more.css")),
        )
        .replace("/* BRAND_STYLE */", include_str!("brand.css")),
    )
}
