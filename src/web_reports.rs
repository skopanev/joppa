//! Agent tokens.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentTokenRequest {
    agent: String,
    /// write (the default) or read.
    access: Option<String>,
}
/// The workspace owner creates an agent token; the raw token is in this reply only.
pub(super) async fn create_agent_token(
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
pub(super) async fn revoke_agent_token(
    State(app): State<App>,
    Path((workspace, id)): Path<(String, i64)>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate_write(&app, &headers).await?;
    check_session_origin(&headers)?;
    result(store::revoke_agent_token(&app.pool, &workspace, &actor, id).await)
}

pub(super) async fn list_agent_tokens(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::list_agent_tokens(&app.pool, &workspace, &actor).await)
}
