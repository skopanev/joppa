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
            "Attach a file of 1 byte to 50 MiB to an object. Send exactly one of content (UTF-8 text) or content_base64 (standard base64). The server puts the bytes in Spaces under their SHA-256 hash, checks the stored size and appends the attach operation. content_type defaults to application/octet-stream. Reuse request_id only for an identical retry. The reply gives the file with a download url, valid 15 minutes.",
            json!({"workspace":text(),"target":text(),"filename":text(),"content_type":text(),"content":text(),"content_base64":text(),"request_id":text()}),
            json!(["workspace", "target", "filename"]),
        ),
        super::tool(
            "joppa_attachments",
            "List the files of an object, newest first: id, name, content_type, size, hash, actor, created_at and a download url, valid 15 minutes. A detached file is not listed.",
            json!({"workspace":text(),"target":text()}),
            json!(["workspace", "target"]),
        ),
    ]
}

fn field<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    a[key].as_str().with_context(|| format!("missing {key}"))
}

/// The current files of `target`. A read of the object checks the access.
async fn files(pool: &Pool, actor: &str, workspace: &str, target: &str) -> Result<Value> {
    let item = store::read_objects(
        pool,
        workspace,
        actor,
        &store::ObjectScope {
            address: Some(target),
            ..Default::default()
        },
    )
    .await?;
    Ok(item["files"].clone())
}

pub async fn attachments(pool: &Pool, actor: &str, a: &Value) -> Result<Value> {
    let (workspace, target) = (field(a, "workspace")?, field(a, "target")?);
    let files = files(pool, actor, workspace, target).await?;
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
    // The read comes BEFORE the upload: a caller without access stores nothing.
    files(pool, actor, workspace, target).await?;
    let (hash, size) = spaces::storage()?.put(&bytes).await?;
    // An identical retry with the same request_id sends the same command, so it replays.
    let request = a["request_id"]
        .as_str()
        .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
    let id = joppa::digest_hex::sha256_hex(&request)[..32].to_owned();
    let command = json!({"op":"attach","target":target,"id":id,"name":filename,
        "content_type":a["content_type"].as_str().unwrap_or(""),"size":size,"hash":hash});
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
