//! joppa_attach and joppa_attachments: files over MCP. The bytes go to Spaces, and the
//! journal gets the attach operation with the hash.
use anyhow::{Context, Result, ensure};
use base64::Engine;
use deadpool_postgres::Pool;
use joppa::{spaces, store};
use serde_json::{Value, json};

pub fn tools() -> [Value; 2] {
    let text = || json!({"type":"string"});
    [
        super::tool(
            "joppa_attach",
            "Attach a file of 1 byte to 50 MiB to an object. Send exactly one of content (UTF-8 text) or content_base64. content_type defaults to application/octet-stream. check_result (pass or fail) makes the file a manual check of a criterion. valid_until (YYYY-MM-DD, UTC) ends a pass. Reuse request_id only for an identical retry. The reply gives the file with a download url, valid 15 minutes.",
            json!({"workspace":text(),"target":text(),"filename":text(),"content_type":text(),"content":text(),"content_base64":text(),"request_id":text(),"check_result":text(),"valid_until":text()}),
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
    let bytes = match (a["content"].as_str(), a["content_base64"].as_str()) {
        (Some(text), None) => text.as_bytes().to_vec(),
        (None, Some(encoded)) => {
            ensure!(
                encoded.len() <= spaces::ATTACHMENT_MAX.div_ceil(3) * 4,
                "an attachment is at most 50 MiB"
            );
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .context("content_base64 is not standard base64")?
        }
        _ => anyhow::bail!("send exactly one of content or content_base64"),
    };
    // The checks come BEFORE the upload: a refused attach stores nothing.
    let check = |key: &str| (!a[key].is_null()).then(|| a[key].as_str().unwrap_or(""));
    let (result, until) = (check("check_result"), check("valid_until"));
    joppa::manual_checks::check_fields(result, until)?;
    // A caller without access stores nothing.
    let item = object(pool, actor, workspace, target).await?;
    if result.is_some() {
        let types = store::tree_schema(pool, workspace, actor, None).await?;
        let kind = item["kind"].as_str().unwrap_or_default();
        let role = &types["schema"]["types"][kind]["role"];
        ensure!(role == "ac", joppa::manual_checks::TARGET_REFUSAL);
    }
    let folder = store::file_workspace(pool, workspace, actor).await?;
    let (hash, size) = spaces::storage()?.put(&folder, &bytes).await?;
    // An identical retry with the same request_id sends the same command, so it replays.
    let request = a["request_id"]
        .as_str()
        .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
    let id = joppa::digest_hex::sha256_hex(&request)[..32].to_owned();
    let mut command = json!({"op":"attach","target":target,"id":id,"name":filename,
        "content_type":a["content_type"].as_str().unwrap_or(""),"size":size,"hash":hash});
    for (key, value) in [("check_result", result), ("valid_until", until)] {
        if let Some(value) = value {
            command[key] = json!(value);
        }
    }
    let reply = store::execute_remote_input(pool, workspace, actor, &request, &command).await?;
    let file = files(pool, actor, workspace, target)
        .await?
        .as_array()
        .and_then(|files| files.iter().find(|f| f["id"] == id.as_str()).cloned())
        .context("the attached file is not listed")?;
    Ok(
        json!({"workspace":workspace,"target":target,"position":reply["position"],
        "replayed":reply["replayed"],"file":file}),
    )
}
