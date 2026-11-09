//! The PUT of an upload link from joppa_attach with upload: true. The signed link is the
//! credential, so the route takes no token or cookie. It stores the bytes and appends the
//! same attach event as joppa_attach.
use super::*;
use crate::protocol::files::put;
use axum::body::{Body, HttpBody};
use joppa::{
    spaces::{ATTACHMENT_MAX, REQUEST_MAX},
    upload_link::{self, Refusal},
};

fn refuse(status: StatusCode, text: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": text })))
}

/// The checks go in this order: the signature, the lifetime, the access, the size. The body
/// is kept only after the access check. A client sees a refusal only after it sent its body,
/// so the server reads a refused body to the end, at most REQUEST_MAX bytes, and drops it.
pub(super) async fn put_upload(
    State(app): State<App>,
    Path(token): Path<String>,
    headers: HeaderMap,
    mut body: Body,
) -> ApiResult {
    let reply = accept(&app, &token, &headers, &mut body).await;
    if reply.is_err() {
        let _ = read(&mut body, 0).await;
    }
    reply
}

async fn accept(app: &App, token: &str, headers: &HeaderMap, body: &mut Body) -> ApiResult {
    let call = upload_link::open(token).map_err(|refusal| match refusal {
        Refusal::Invalid => refuse(StatusCode::FORBIDDEN, "upload link is not valid"),
        Refusal::Expired => refuse(StatusCode::GONE, "upload link expired"),
    })?;
    put::check(&app.pool, &call).await.map_err(failure)?;
    let too_big = || {
        refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            "an attachment is at most 50 MiB",
        )
    };
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
    if length.is_some_and(|n| n > ATTACHMENT_MAX as u64) {
        return Err(too_big());
    }
    let bytes = read(body, ATTACHMENT_MAX)
        .await
        .map_err(|_| refuse(StatusCode::BAD_REQUEST, "the upload did not finish"))?
        .ok_or_else(too_big)?;
    // The alias of the joppa_attach call names the agent of this write too.
    let work = put::store_bytes(&app.pool, &call, &bytes);
    result(joppa::actor_alias::scope(call.alias.clone(), work).await)
}

/// Read the body up to REQUEST_MAX bytes. Keep at most `keep` bytes, and give None when the
/// body has more.
async fn read(body: &mut Body, keep: usize) -> std::result::Result<Option<Vec<u8>>, axum::Error> {
    let (mut bytes, mut total) = (Vec::new(), 0);
    while total <= REQUEST_MAX
        && let Some(frame) =
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_frame(cx)).await
    {
        let Ok(data) = frame?.into_data() else {
            continue;
        };
        total += data.len();
        if total <= keep {
            bytes.extend_from_slice(&data);
        }
    }
    Ok((total <= keep).then_some(bytes))
}
