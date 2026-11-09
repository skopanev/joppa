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
    let text = || json!({"type":"string"});
    [
        super::tool(
            "joppa_attach",
            "Attach a file of 1 byte to 50 MiB to an object. Send exactly one of content (UTF-8 text), content_base64, url (an HTTPS GET of a public address) or upload: true. upload gives upload_url: PUT the raw bytes there in 15 minutes, with no token. content_type defaults to the url reply type, else application/octet-stream. check_result (pass or fail) makes the file a manual check of a criterion. valid_until (YYYY-MM-DD, UTC) ends a pass. Reuse request_id only for an identical retry. The reply gives the file with a download url, valid 15 minutes.",
            json!({"workspace":text(),"target":text(),"filename":text(),"content_type":text(),"content":text(),"content_base64":text(),"url":text(),"upload":{"type":"boolean"},"request_id":text(),"check_result":{"type":"string","enum":["pass","fail"]},"valid_until":text()}),
            json!(["workspace", "target", "filename"]),
        ),
        super::tool(
            "joppa_attachments",
            "List the files of an object, newest first: id, name, content_type, size, hash, actor, created_at and a download url, valid 15 minutes. removed: true also lists the files detached in the last 14 days, with removed: true and removed_at.",
            json!({"workspace":text(),"target":text(),"removed":{"type":"boolean"}}),
            json!(["workspace", "target"]),
        ),
    ]
}

fn field<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    a[key].as_str().with_context(|| format!("missing {key}"))
}

/// The current object `target` with its files. A read of the object checks the access.
async fn object(pool: &Pool, actor: &str, workspace: &str, target: &str) -> Result<Value> {
    store::read_objects(
        pool,
        workspace,
        actor,
        &store::ObjectScope {
            address: Some(target),
            ..Default::default()
        },
    )
    .await
}

async fn files(pool: &Pool, actor: &str, workspace: &str, target: &str) -> Result<Value> {
    Ok(object(pool, actor, workspace, target).await?["files"].clone())
}

/// The files of `item` detached in the last 14 days, newest detach first, each with a
/// download url. A detach without removed_at is older than the 14-day rule, so the reconcile
/// never deletes its file, and the list keeps it.
fn removed(item: &Value, folder: &str) -> Result<Vec<Value>> {
    let since = chrono::Utc::now() - chrono::TimeDelta::days(14);
    let at = |file: &Value| {
        let text = file["removed_at"].as_str().unwrap_or("");
        chrono::DateTime::parse_from_rfc3339(text)
            .or_else(|_| chrono::DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f%#z"))
            .ok()
    };
    let mut files: Vec<_> = item["annotations"]["attachments"]
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
            let text = |key: &str| file[key].as_str().unwrap_or("").to_owned();
            let key = spaces::Spaces::key(folder, &text("hash"))?;
            let url = spaces::storage()?.download_url(&key, &text("name"), &text("content_type"));
            file["id"] = json!(id);
            file["created_at"] = file["at"].clone();
            file["url"] = json!(url);
            Ok(file)
        })
        .collect()
}

pub async fn attachments(pool: &Pool, actor: &str, a: &Value) -> Result<Value> {
    let (workspace, target) = (field(a, "workspace")?, field(a, "target")?);
    let with_removed = match &a["removed"] {
        Value::Null => false,
        value => value.as_bool().context("removed must be a boolean")?,
    };
    let item = object(pool, actor, workspace, target).await?;
    let mut files = item["files"].as_array().cloned().unwrap_or_default();
    if with_removed {
        let folder = store::file_workspace(pool, workspace, actor).await?;
        files.extend(removed(&item, &folder)?);
    }
    Ok(json!({"workspace":workspace,"target":target,"files":files}))
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
