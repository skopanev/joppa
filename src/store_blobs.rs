//! Content-addressed attachment blobs.
use super::*;

/// Предел одного вложения. Количество не ограничено — их столько, сколько
/// нужно; ограничен РАЗМЕР, потому что байты лежат в той же базе, что и
/// журнал, и её резервная копия снимается целиком.
pub const ATTACHMENT_MAX: usize = 10 * 1024 * 1024;

/// Положить файл и вернуть его отпечаток.
///
/// Отпечаток — sha256 содержимого, поэтому повторная загрузка того же файла
/// ничего не добавляет: она возвращает тот же ключ. Проверки доступа здесь
/// нет намеренно — хранилище не знает про воркспейсы; правом распоряжается
/// то место, где отпечаток попадает в журнал.
pub async fn put_blob(pool: &Pool, bytes: &[u8]) -> Result<(String, usize)> {
    use sha2::{Digest, Sha256};
    ensure!(!bytes.is_empty(), "an empty file carries nothing");
    ensure!(
        bytes.len() <= ATTACHMENT_MAX,
        "an attachment is at most {} MB",
        ATTACHMENT_MAX / (1024 * 1024)
    );
    let hash = format!("{:x}", Sha256::digest(bytes));
    pool.get()
        .await?
        .execute(
            "INSERT INTO joppa.blobs(hash,bytes,size) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
            &[&hash, &bytes, &(bytes.len() as i64)],
        )
        .await?;
    Ok((hash, bytes.len()))
}

/// Выдать файл — но только тому, кто видит объект, к которому он приложен.
///
/// Отпечаток глобален: один и тот же файл может лежать в двух воркспейсах.
/// Поэтому право проверяется не по файлу, а по УПОМИНАНИЮ: этот отпечаток
/// должен встречаться в заметках ЭТОГО воркспейса. Иначе знание отпечатка
/// было бы паролем, а он вычисляется из содержимого.
pub async fn get_blob(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    hash: &str,
) -> Result<(String, String, Vec<u8>)> {
    let conn = pool.get().await?;
    let resolved = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &resolved, actor, false).await?;
    let found = conn.query_opt("SELECT a.value->>'name',a.value->>'content_type' FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'attachments') a WHERE n.workspace=$1 AND n.section='notes' AND a.value->>'hash'=$2 LIMIT 1", &[&resolved,&hash]).await?.ok_or(AccessDenied)?;
    let row = conn
        .query_opt("SELECT bytes FROM joppa.blobs WHERE hash=$1", &[&hash])
        .await?
        .ok_or(AccessDenied)?;
    Ok((found.get(0), found.get(1), row.get(0)))
}
