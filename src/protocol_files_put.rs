//! The end of each joppa_attach: the checks, the upload link and the store of the bytes with
//! the attach event. The PUT of an upload link (web_uploads.rs) runs the same steps.
use super::*;
use joppa::upload_link::{self, Upload};

/// Refuse bad check fields, a caller without access, or a check on a node that is not a
/// criterion.
pub(crate) async fn check(pool: &Pool, call: &Upload) -> Result<()> {
    let (result, until) = (call.check_result.as_deref(), call.valid_until.as_deref());
    joppa::manual_checks::check_fields(result, until)?;
    let item = object(pool, &call.actor, &call.workspace, &call.target).await?;
    if result.is_some() {
        let types = store::tree_schema(pool, &call.workspace, &call.actor, None).await?;
        let kind = item["kind"].as_str().unwrap_or_default();
        let role = &types["schema"]["types"][kind]["role"];
        ensure!(role == "ac", joppa::manual_checks::TARGET_REFUSAL);
    }
    Ok(())
}

/// The signed upload link of `call` on this server. It stores nothing yet.
pub(crate) fn link(mut call: Upload) -> Result<Value> {
    let origin = std::env::var("JOPPA_PUBLIC_URL")
        .ok()
        .context("Public server URL is not configured")?;
    let origin = reqwest::Url::parse(&origin)?.origin().ascii_serialization();
    call.expires = chrono::Utc::now().timestamp() + upload_link::lifetime()?;
    let expires_at = chrono::DateTime::from_timestamp(call.expires, 0)
        .context("the link lifetime is out of range")?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let token = upload_link::sign(&call)?;
    Ok(json!({"workspace":call.workspace,"target":call.target,
        "upload_url":format!("{origin}/api/uploads/{token}"),"expires_at":expires_at}))
}

/// Store `bytes` in Spaces and append the attach event of `call`.
pub(crate) async fn store_bytes(pool: &Pool, call: &Upload, bytes: &[u8]) -> Result<Value> {
    let (workspace, target) = (call.workspace.as_str(), call.target.as_str());
    ensure!(!bytes.is_empty(), "an empty file carries nothing");
    let folder = store::file_workspace(pool, workspace, &call.actor).await?;
    // A reused request_id with other bytes is refused BEFORE the bytes go to Spaces.
    let digest = joppa::digest_hex::sha256_hex(bytes);
    if let Some((actor, hash)) = store::receipt_hash(pool, &folder, &call.request_id).await? {
        ensure!(
            actor == call.actor && hash == digest,
            "request_id reused with different content/actor"
        );
    }
    let (hash, size) = spaces::storage()?.put(&folder, bytes).await?;
    let id = joppa::digest_hex::sha256_hex(&call.request_id)[..32].to_owned();
    let mut command = json!({"op":"attach","target":target,"id":id,"name":call.filename,
        "content_type":call.content_type,"size":size,"hash":hash});
    for (key, value) in [
        ("check_result", &call.check_result),
        ("valid_until", &call.valid_until),
    ] {
        if let Some(value) = value {
            command[key] = json!(value);
        }
    }
    let reply =
        store::execute_remote_input(pool, workspace, &call.actor, &call.request_id, &command)
            .await?;
    let file = files(pool, &call.actor, workspace, target)
        .await?
        .as_array()
        .and_then(|files| files.iter().find(|f| f["id"] == id.as_str()).cloned())
        .context("the attached file is not listed")?;
    Ok(
        json!({"workspace":workspace,"target":target,"position":reply["position"],
        "replayed":reply["replayed"],"file":file}),
    )
}
