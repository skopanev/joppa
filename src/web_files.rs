//! File attachments: upload and download.
use super::*;

/// Приём файла: сырые байты, имя и тип — в заголовках.
///
/// Ответ несёт отпечаток и размер; само вложение появляется в журнале
/// отдельной командой Attach. Разделено намеренно: загрузка может не дойти
/// до записи (закрыли вкладку), и тогда в базе остаётся неприкаянный файл,
/// а не запись о вложении, которое нельзя открыть.
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
pub(super) fn percent(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}
pub(super) async fn download(
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
