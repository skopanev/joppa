//! The roles of the people of a workspace in Settings: the role of a new email grant, and a
//! role change of a member or an invited email. MCP has none of these routes.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmailGrantRole {
    pub(super) workspace: String,
    pub(super) email: String,
    /// viewer (Read-only, the default) or human (Editor).
    #[serde(default)]
    pub(super) role: Option<String>,
}
/// A human member grants a workspace to a Google email from a signed-in browser.
pub(super) async fn grant_email_role(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<EmailGrantRole>,
) -> ApiResult {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(auth_error("Sign in with the browser to grant access"));
    }
    let actor = authenticate(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let role = body.role.as_deref().unwrap_or(joppa::commands::VIEWER);
    result(store::grant_email_role(&app.pool, &body.workspace, &actor, &body.email, role).await)
}
/// An email grant without a role, as the tests send it. The route takes EmailGrantRole.
#[cfg(test)]
pub(super) struct EmailGrant {
    pub(super) workspace: String,
    pub(super) email: String,
}
/// The grant of the route with the default role Read-only.
#[cfg(test)]
pub(super) async fn grant_email(
    app: State<App>,
    headers: HeaderMap,
    Json(body): Json<EmailGrant>,
) -> ApiResult {
    let (workspace, email, role) = (body.workspace, body.email, None);
    grant_email_role(
        app,
        headers,
        Json(EmailGrantRole {
            workspace,
            email,
            role,
        }),
    )
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RoleChange {
    workspace: String,
    #[serde(default)]
    member: Option<String>,
    #[serde(default)]
    email: Option<String>,
    role: String,
}
/// A human member sets the role of a member or an invited email from a signed-in browser.
pub(super) async fn change_role(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<RoleChange>,
) -> ApiResult {
    // A credential of an agent or of MCP never changes a role (ac:lg5zo4gu).
    if headers.contains_key(header::AUTHORIZATION) {
        let text = "Sign in with the browser to change a role";
        return Err((StatusCode::UNAUTHORIZED, Json(json!({ "error": text }))));
    }
    let actor = authenticate(&app, &headers).await?;
    web_auth::check_origin(&headers).map_err(auth_error)?;
    let (member, email) = (body.member.as_deref(), body.email.as_deref());
    result(
        store::set_role(
            &app.pool,
            &body.workspace,
            &actor,
            member,
            email,
            &body.role,
        )
        .await,
    )
}
