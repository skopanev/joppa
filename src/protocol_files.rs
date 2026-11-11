//! joppa_attach and joppa_attachments: files over MCP. The bytes go to Spaces, and the
//! journal gets the attach operation with the hash.
use anyhow::{Context, Result, ensure};
use base64::Engine;
use deadpool_postgres::Pool;
use joppa::{spaces, store, upload_link::Upload};
use serde_json::{Value, json};
#[path = "protocol_files_put.rs"]
pub(crate) mod put;

pub fn tools() -> [Value; 2] {
    use super::schemas::{CURSOR, WORKSPACE, text};
    [
        super::tool(
            "joppa_attach",
            "Attach a file of 1 byte to 50 MiB to an object. Send exactly one of content, content_base64, url or upload. The reply gives the file with a download url, valid 15 minutes.",
            json!({"workspace":text(WORKSPACE),"target":text("object address"),"filename":text("file name"),
                "content_type":text("default: the url type, else application/octet-stream"),
                "content":text("UTF-8 text"),"content_base64":text("the bytes in base64"),
                "url":text("public HTTPS address to GET"),
                "upload":{"type":"boolean","description":"true gives upload_url: PUT the bytes there in 15 minutes"},
                "request_id":text("the same value only for an identical retry"),
                "check_result":{"type":"string","enum":["pass","fail"],"description":"a manual check of a criterion"},
                "valid_until":text("YYYY-MM-DD (UTC): the end of a pass")}),
            json!(["workspace", "target", "filename"]),
        ),
        super::tool(
            "joppa_attachments",
            "List the files of an object, newest first, with a download url, valid 15 minutes. A reply over 256 KB is refused.",
            json!({"workspace":text(WORKSPACE),"target":text("object address"),
                "removed":{"type":"boolean","description":"also the files detached in the last 14 days"},
                "limit":{"type":"integer","minimum":1,"maximum":100,"description":"1 to 100; without it, all files"},
                "cursor":text(CURSOR)}),
            json!(["workspace", "target"]),
        ),
    ]
}

fn field<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    a[key].as_str().with_context(|| format!("missing {key}"))
}

/// The files of `item` (store::object_files) detached in the last 14 days, newest detach
/// first. A detach without removed_at is older than the 14-day rule, so the reconcile never
/// deletes its file, and the list keeps it.
fn removed(item: &Value) -> Vec<Value> {
    let since = chrono::Utc::now() - chrono::TimeDelta::days(14);
    let at = |file: &Value| {
        let text = file["removed_at"].as_str().unwrap_or("");
        chrono::DateTime::parse_from_rfc3339(text)
            .or_else(|_| chrono::DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f%#z"))
            .ok()
    };
    let mut files: Vec<_> = item["attachments"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, file)| file["removed"] == true)
        .map(|(id, file)| (at(file), id, file))
        .filter(|(at, _, file)| at.is_some_and(|at| at > since) || file["removed_at"].is_null())
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    files
        .into_iter()
        .map(|(_, id, file)| {
            let mut file = file.clone();
            file["id"] = json!(id);
            file["created_at"] = file["at"].clone();
            file
        })
        .collect()
}

/// The page of `files` after the file id `cursor`: at most `limit` files, and the id of the
/// last one as next_cursor when more files follow.
fn file_page(
    mut files: Vec<Value>,
    limit: Option<usize>,
    cursor: Option<&str>,
) -> Result<(Vec<Value>, Option<String>)> {
    let start = match cursor {
        None => 0,
        Some(id) => {
            files
                .iter()
                .position(|f| f["id"] == id)
                .context("invalid file cursor; start again without cursor")?
                + 1
        }
    };
    let end = limit.map_or(files.len(), |n| (start + n).min(files.len()));
    let more = end < files.len();
    let page: Vec<Value> = files.drain(start..end).collect();
    let next = more.then(|| {
        page.last()
            .map(|f| f["id"].as_str().unwrap_or("").to_owned())
    });
    Ok((page, next.flatten()))
}

/// joppa_attachments: the attachments of the target, read directly, newest first, each with
/// a download url. Only the files of the page are signed.
pub async fn attachments(pool: &Pool, actor: &str, a: &Value) -> Result<Value> {
    let (workspace, target) = (field(a, "workspace")?, field(a, "target")?);
    let with_removed = match &a["removed"] {
        Value::Null => false,
        value => value.as_bool().context("removed must be a boolean")?,
    };
    let limit = match &a["limit"] {
        Value::Null => None,
        value => Some(
            value
                .as_u64()
                .filter(|n| (1..=100).contains(n))
                .context("limit must be 1 to 100")? as usize,
        ),
    };
    let cursor = match &a["cursor"] {
        Value::Null => None,
        value => Some(value.as_str().context("cursor must be a string")?),
    };
    let item = store::object_files(pool, workspace, actor, target).await?;
    let mut files = joppa::file_list::present(&item["attachments"]);
    if with_removed {
        files.extend(removed(&item));
    }
    let (mut files, next) = file_page(files, limit, cursor)?;
    joppa::file_list::sign(item["folder"].as_str().unwrap_or_default(), &mut files)?;
    let mut reply = json!({"workspace":workspace,"target":target,"files":files});
    if let Some(next) = next {
        reply["next_cursor"] = json!(next);
    }
    Ok(reply)
}

pub async fn attach(pool: &Pool, actor: &str, a: &Value) -> Result<Value> {
    let (workspace, target) = (field(a, "workspace")?, field(a, "target")?);
    let filename = field(a, "filename")?;
    let text = |key: &str| a[key].as_str().map(str::to_owned);
    let upload = match &a["upload"] {
        Value::Null => false,
        value => value.as_bool().context("upload must be a boolean")?,
    };
    let sources = ["content", "content_base64", "url"];
    let count = sources.iter().filter(|k| a[**k].is_string()).count() + usize::from(upload);
    ensure!(
        count == 1,
        "send exactly one of content, content_base64, url or upload"
    );
    let decoded = match a["content_base64"].as_str() {
        Some(encoded) => {
            ensure!(
                encoded.len() <= spaces::ATTACHMENT_MAX.div_ceil(3) * 4,
                "an attachment is at most 50 MiB"
            );
            let engine = base64::engine::general_purpose::STANDARD;
            Some(
                engine
                    .decode(encoded)
                    .context("content_base64 is not standard base64")?,
            )
        }
        None => None,
    };
    let check = |key: &str| (!a[key].is_null()).then(|| text(key).unwrap_or_default());
    // An identical retry with the same request_id sends the same command, so it replays.
    let mut call = Upload {
        actor: actor.into(),
        alias: joppa::actor_alias::current(),
        workspace: workspace.into(),
        target: target.into(),
        filename: filename.into(),
        content_type: text("content_type").unwrap_or_default(),
        check_result: check("check_result"),
        valid_until: check("valid_until"),
        request_id: text("request_id").unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        expires: 0,
    };
    // The checks come BEFORE the upload or the fetch: a refused attach stores nothing.
    put::check(pool, &call).await?;
    if upload {
        return put::link(call);
    }
    let bytes = match (text("content"), decoded, text("url")) {
        (Some(content), _, _) => content.into_bytes(),
        (_, Some(bytes), _) => bytes,
        (_, _, Some(url)) => {
            let (bytes, sent) = joppa::attach_fetch::fetch(&url).await?;
            if call.content_type.is_empty() {
                call.content_type = sent.unwrap_or_default();
            }
            bytes
        }
        _ => unreachable!("one source is present"),
    };
    put::store_bytes(pool, &call, &bytes).await
}
