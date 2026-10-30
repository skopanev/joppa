//! A local stand-in for DigitalOcean Spaces: path-style PUT, GET, HEAD and DELETE of
//! `/<bucket>/files/<hash>`. It refuses a request with a wrong or expired signature.
#![allow(dead_code)]
use axum::{
    Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

pub const BUCKET: &str = "joppa-test";
const REGION: &str = "test-1";
const KEY_ID: &str = "test-key-id";
const SECRET: &str = "test-secret-key";

#[derive(Clone, Default)]
pub struct Fake {
    pub endpoint: String,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

impl Fake {
    pub fn has(&self, hash: &str) -> bool {
        self.files.lock().unwrap().contains_key(hash)
    }
    /// The stored hashes, in order.
    pub fn hashes(&self) -> Vec<String> {
        self.files.lock().unwrap().keys().cloned().collect()
    }
    pub fn bytes(&self, hash: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(hash).cloned()
    }
    /// The variables of storage.env for a server process.
    pub fn env(&self) -> [(&'static str, String); 5] {
        [
            ("SPACES_ENDPOINT", self.endpoint.clone()),
            ("SPACES_BUCKET", BUCKET.into()),
            ("SPACES_REGION", REGION.into()),
            ("AWS_ACCESS_KEY_ID", KEY_ID.into()),
            ("AWS_SECRET_ACCESS_KEY", SECRET.into()),
        ]
    }
    /// The file storage of this test process, for tests that call the store directly.
    pub fn configure(&self) {
        let spaces = joppa::spaces::Spaces::new(&self.endpoint, BUCKET, REGION, KEY_ID, SECRET);
        let _ = joppa::spaces::configure(spaces.unwrap());
    }
}

/// The one fake of this test process. It runs on its own thread, so it outlives the
/// runtime of each test.
pub fn start() -> &'static Fake {
    static FAKE: OnceLock<Fake> = OnceLock::new();
    FAKE.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let fake = Fake {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            ..Default::default()
        };
        let state = fake.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let app = Router::new()
                    .route("/{bucket}/files/{hash}", get(file).put(file).delete(file))
                    .layer(axum::extract::DefaultBodyLimit::disable())
                    .with_state(state);
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        });
        fake
    })
}

/// The signature check of Spaces: the same canonical request with the same time and
/// lifetime gives the same signature, and the lifetime has not ended.
fn signed(method: &Method, host: &str, path: &str, query: &BTreeMap<String, String>) -> bool {
    let (Some(time), Some(ttl), Some(signature)) = (
        query.get("X-Amz-Date"),
        query
            .get("X-Amz-Expires")
            .and_then(|t| t.parse::<u32>().ok()),
        query.get("X-Amz-Signature"),
    ) else {
        return false;
    };
    let Ok(at) = chrono::NaiveDateTime::parse_from_str(time, "%Y%m%dT%H%M%SZ") else {
        return false;
    };
    let age = chrono::Utc::now().naive_utc() - at;
    let extra: Vec<(&str, &str)> = query
        .iter()
        .filter(|(k, _)| k.starts_with("response-"))
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let signer = joppa::spaces_sign::Signer {
        key_id: KEY_ID,
        secret: SECRET,
        region: REGION,
    };
    let expected = signer.query(method.as_str(), host, path, time, ttl, &extra);
    age.num_seconds() <= i64::from(ttl)
        && expected.ends_with(&format!("X-Amz-Signature={signature}"))
}

async fn file(
    State(fake): State<Fake>,
    Path((bucket, hash)): Path<(String, String)>,
    Query(query): Query<BTreeMap<String, String>>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let path = format!("/{bucket}/files/{hash}");
    // Spaces refuses a second auth mechanism next to the presigned query.
    let auth = headers.contains_key(header::AUTHORIZATION);
    if auth || bucket != BUCKET || !signed(&method, host, &path, &query) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut files = fake.files.lock().unwrap();
    match method {
        Method::PUT => {
            files.insert(hash, body.to_vec());
            StatusCode::OK.into_response()
        }
        Method::DELETE => {
            files.remove(&hash);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => match files.get(&hash) {
            None => StatusCode::NOT_FOUND.into_response(),
            Some(bytes) => {
                let header = |key: &str| query.get(key).cloned().unwrap_or_default();
                (
                    [
                        (header::CONTENT_TYPE, header("response-content-type")),
                        (
                            header::CONTENT_DISPOSITION,
                            header("response-content-disposition"),
                        ),
                    ],
                    bytes.clone(),
                )
                    .into_response()
            }
        },
    }
}
