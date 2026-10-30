//! File attachments: upload and download. The bytes live in Spaces (joppa::spaces).
use super::*;
use joppa::spaces;

/// Приём файла: сырые байты, имя и тип — в заголовках.
///
/// Ответ несёт отпечаток и размер; само вложение появляется в журнале
/// отдельной командой Attach. Разделено намеренно: загрузка может не дойти
/// до записи (закрыли вкладку), и тогда в хранилище остаётся неприкаянный
/// файл, а не запись о вложении, которое нельзя открыть.
pub(super) async fn upload(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult {
    require_write_credential(&headers)?;
    let actor = authenticate_write(&app, &headers).await?;
    // Право проверяется ДО записи файла: иначе чужой смог бы наполнять
    // хранилище, ничего не имея в воркспейсе.
    let folder = store::file_workspace(&app.pool, &workspace, &actor)
        .await
        .map_err(failure)?;
    if body.len() > spaces::ATTACHMENT_MAX {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"error":"an attachment is at most 50 MiB"})),
        ));
    }
    result(
        async { spaces::storage()?.put(&folder, &body).await }
            .await
            .map(|(hash, size)| json!({"hash": hash, "size": size})),
    )
}

/// A member gets a presigned GET of Spaces, valid 15 minutes, with the name and the
/// content type of the attachment.
pub(super) async fn download(
    State(app): State<App>,
    Path((workspace, hash)): Path<(String, String)>,
    headers: HeaderMap,
) -> std::result::Result<axum::response::Redirect, (StatusCode, Json<Value>)> {
    let actor = authenticate(&app, &headers).await?;
    let found = store::attachment(&app.pool, &workspace, &actor, &hash).await;
    let storage = spaces::storage().map_err(failure)?;
    match found {
        Ok((key, name, content_type)) => Ok(axum::response::Redirect::temporary(
            &storage.download_url(&key, &name, &content_type),
        )),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"no such attachment here"})),
        )),
    }
}
