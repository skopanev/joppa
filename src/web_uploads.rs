//! The PUT of an upload link from joppa_attach with upload: true. The signed link is the
//! credential, so the route takes no token or cookie. It stores the bytes and appends the
//! same attach event as joppa_attach.
use super::*;
use crate::protocol::files::put;
use axum::body::{Body, Bytes, HttpBody};
use joppa::{
    spaces::{ATTACHMENT_MAX, REQUEST_MAX},
    upload_link::{self, Refusal},
};
use std::time::Duration;

fn refuse(status: StatusCode, text: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": text })))
}

/// The checks go in this order: the signature, the lifetime, the access, the size. The body
/// is kept only after the access check. A client sees a refusal only after it sent its body,
/// so the server reads the rest of a refused body, 30 seconds and REQUEST_MAX bytes at most,
/// and drops it.
pub(super) async fn put_upload(
    State(app): State<App>,
    Path(token): Path<String>,
    headers: HeaderMap,
    mut body: Body,
) -> ApiResult {
    let mut total = 0;
    let reply = accept(&app, &token, &headers, &mut body, &mut total).await;
    if reply.is_err() && !body.is_end_stream() {
        let _ = tokio::time::timeout(Duration::from_secs(30), drain(&mut body, total)).await;
    }
    reply
}

async fn accept(
    app: &App,
    token: &str,
    headers: &HeaderMap,
    body: &mut Body,
    total: &mut usize,
) -> ApiResult {
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
    let bytes = read(body, total)
        .await
        .map_err(|_| refuse(StatusCode::BAD_REQUEST, "the upload did not finish"))?
        .ok_or_else(too_big)?;
    // The alias of the joppa_attach call names the agent of this write too.
    let work = put::store_bytes(&app.pool, &call, &bytes);
    result(joppa::actor_alias::scope(call.alias.clone(), work).await)
}

/// The next frame of the body: its data, or None for a frame without data.
async fn frame(body: &mut Body) -> Option<std::result::Result<Option<Bytes>, axum::Error>> {
    let next = std::future::poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_frame(cx)).await;
    next.map(|frame| frame.map(|frame| frame.into_data().ok()))
}

/// The whole body, or None when it has more than 50 MiB. `total` counts the bytes read.
async fn read(
    body: &mut Body,
    total: &mut usize,
) -> std::result::Result<Option<Vec<u8>>, axum::Error> {
    let mut bytes = Vec::new();
    while let Some(next) = frame(body).await {
        let Some(data) = next? else {
            continue;
        };
        *total += data.len();
        if *total > ATTACHMENT_MAX {
            return Ok(None);
        }
        bytes.extend_from_slice(&data);
    }
    Ok(Some(bytes))
}

/// Read and drop the rest of the body, until REQUEST_MAX bytes in all.
async fn drain(body: &mut Body, mut total: usize) {
    while total <= REQUEST_MAX
        && let Some(Ok(next)) = frame(body).await
    {
        total += next.map_or(0, |data| data.len());
    }
}
