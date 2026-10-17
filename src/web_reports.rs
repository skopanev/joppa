//! Test report upload, criterion test status and agent tokens.
use super::*;

/// A test run uploads its JUnit report. Only a token may upload, never a browser session.
pub(super) async fn upload_report(
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
pub(super) struct TestsQuery {
    /// One criterion address, or several separated by commas. Without it: the overview.
    ac: Option<String>,
    /// A task address: done while its criterion is green.
    task: Option<String>,
}
pub(super) async fn criterion_tests(
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
