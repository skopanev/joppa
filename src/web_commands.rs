//! Write routes: commands, dependencies and the tree order.
use super::*;

pub(super) async fn command(
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
/// The board is read-only; dependency changes go through Joppa MCP.
pub(super) async fn dependency() -> ApiResult {
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({"error":"The portal is read-only. Use Joppa MCP to make changes."})),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrderRequest {
    request_id: String,
    kind: String,
    parent: Option<String>,
    expected: Vec<String>,
    ids: Vec<String>,
}
pub(super) async fn reorder(
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
