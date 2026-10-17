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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DependencyRequest {
    request_id: String,
    source: String,
    target: String,
    action: String,
}
/// A signed-in member adds or removes one blocking dependency from the board. Only a
/// browser session from this origin with a write grant may; commands and reorder stay
/// closed to cookie sessions.
pub(super) async fn dependency(
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
