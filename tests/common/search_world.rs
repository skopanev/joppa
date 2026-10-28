//! A test embedding provider and a real server that uses it. The provider gives each text a
//! unit vector on the dimension of the first keyword that the text holds, so tests choose
//! which texts are close. It counts requests and open requests, can hold the first request,
//! and answers 400 for a text with "rejectme", 500 for a text with "flakyword", and 503 for
//! every text while it is down. It keeps the texts of each request, and it can answer 500
//! to each request with more than one text.
#![allow(dead_code)]
use anyhow::{Context, Result, ensure};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// The keyword of each vector dimension. A text without a keyword gets the last dimension.
pub const KEYWORDS: [&[&str]; 4] = [
    &["renewal", "extend membership"],
    &["invoice"],
    &["alpha"],
    &["beta"],
];

#[derive(Clone, Default)]
pub struct Fake {
    pub calls: Arc<AtomicUsize>,
    pub open: Arc<AtomicUsize>,
    pub max_open: Arc<AtomicUsize>,
    pub starts: Arc<Mutex<Vec<Instant>>>,
    pub down: Arc<AtomicBool>,
    pub delay_ms: Arc<AtomicU64>,
    pub hold_first: Arc<AtomicBool>,
    pub held: Arc<Notify>,
    pub release: Arc<Notify>,
    pub requests: Arc<Mutex<Vec<Vec<String>>>>,
    pub refuse_batches: Arc<AtomicBool>,
}

struct Open(Arc<AtomicUsize>);
impl Drop for Open {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn dimension(text: &str) -> usize {
    let text = text.to_lowercase();
    KEYWORDS
        .iter()
        .position(|words| words.iter().any(|w| text.contains(w)))
        .unwrap_or(KEYWORDS.len())
}

async fn embed(State(f): State<Fake>, Json(v): Json<Value>) -> (StatusCode, Json<Value>) {
    f.calls.fetch_add(1, Ordering::SeqCst);
    let now = f.open.fetch_add(1, Ordering::SeqCst) + 1;
    let _open = Open(f.open.clone());
    f.max_open.fetch_max(now, Ordering::SeqCst);
    f.starts.lock().unwrap().push(Instant::now());
    if f.hold_first.swap(false, Ordering::SeqCst) {
        f.held.notify_one();
        f.release.notified().await;
    }
    let delay = f.delay_ms.load(Ordering::SeqCst);
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    if f.down.load(Ordering::SeqCst) {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
    }
    let sent: Vec<String> = v["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    f.requests.lock().unwrap().push(sent.clone());
    if sent.len() > 1 && f.refuse_batches.load(Ordering::SeqCst) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({})));
    }
    let inputs: Vec<String> = sent.iter().map(|s| s.to_lowercase()).collect();
    if inputs.iter().any(|s| s.contains("rejectme")) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"input"})));
    }
    if inputs.iter().any(|s| s.contains("flakyword")) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({})));
    }
    let data = inputs
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let mut vector = vec![0.0; 4096];
            vector[dimension(text)] = 1.0;
            json!({"index":index,"embedding":vector})
        })
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(json!({"data":data})))
}

/// Start the provider. Returns it and its endpoint URL.
pub async fn provider() -> Result<(Fake, String)> {
    let fake = Fake::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new()
        .route("/embeddings", post(embed))
        .with_state(fake.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Ok((fake, format!("http://{addr}/embeddings")))
}

pub struct Server {
    child: Child,
    pub root: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The bearer token of each test actor.
pub fn token(actor: &str) -> String {
    format!("search-test-token-{actor}-0123456789")
}

fn command(db: &str, actors: &[&str], key: Option<&str>, endpoint: &str) -> Result<(Command, u16)> {
    let tokens = std::env::temp_dir().join(format!("joppa-search-{}.json", uuid::Uuid::new_v4()));
    let map: serde_json::Map<String, Value> = actors.iter().map(|a| (token(a), json!(a))).collect();
    std::fs::write(&tokens, Value::Object(map).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_joppa"));
    cmd.args(["serve", &port.to_string()])
        .env("JOPPA_DATABASE_URL", db)
        .env("JOPPA_TOKENS_FILE", tokens)
        .env("JOPPA_EMBEDDING_URL", endpoint)
        .env_remove("JOPPA_LOGIN_USERS_FILE")
        .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
        .env_remove("JOPPA_ACCESS_RULES_FILE")
        .stdout(Stdio::null());
    match key {
        Some(key) => cmd.env("DEEPINFRA_API_KEY", key),
        None => cmd.env_remove("DEEPINFRA_API_KEY"),
    };
    Ok((cmd, port))
}

/// Start a server with the provider key and the test provider endpoint.
pub async fn serve(db: &str, actors: &[&str], endpoint: &str) -> Result<Server> {
    let (mut cmd, port) = command(db, actors, Some("test-only-key"), endpoint)?;
    let server = Server {
        child: cmd.stderr(Stdio::null()).spawn()?,
        root: format!("http://127.0.0.1:{port}"),
    };
    for _ in 0..200 {
        if reqwest::get(format!("{}/health", server.root))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(server);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("test server did not start")
}

/// Start a server without the provider key. Returns its exit status and its error text.
pub fn serve_without_key(db: &str) -> Result<(std::process::ExitStatus, String)> {
    let out = command(db, &["owner"], None, "http://127.0.0.1:9/embeddings")?
        .0
        .stderr(Stdio::piped())
        .output()?;
    Ok((
        out.status,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// Call an MCP tool as `actor`. Returns the JSON reply or the tool error.
pub async fn mcp(root: &str, actor: &str, name: &str, args: Value) -> Result<Value> {
    let body: Value = reqwest::Client::new()
        .post(format!("{root}/mcp"))
        .bearer_auth(token(actor))
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":(legacy_read_args(name, &args))}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .with_context(|| format!("MCP reply: {body}"))?;
    ensure!(body["result"]["isError"] != true, "{name}: {text}");
    Ok(serde_json::from_str(text)?)
}

/// Search over the HTTP route of the board as `actor`.
pub async fn http_search(
    root: &str,
    actor: &str,
    ws: &str,
    query: &[(&str, &str)],
) -> Result<Value> {
    Ok(reqwest::Client::new()
        .get(format!("{root}/api/workspaces/{ws}/search"))
        .bearer_auth(token(actor))
        .query(query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Legacy assertions read the earlier full shape of the read tools (requirement:read-contract).
#[allow(dead_code)]
fn legacy_read_args(tool: &str, args: &serde_json::Value) -> serde_json::Value {
    let mut args = args.clone();
    let read = [
        "joppa_objects",
        "joppa_read",
        "joppa_tests",
        "joppa_search",
        "joppa_timeline",
        "joppa_tasks",
        "joppa_history",
    ];
    if read.contains(&tool)
        && args.get("verbose").is_none()
        && let Some(map) = args.as_object_mut()
    {
        map.insert("verbose".into(), serde_json::json!(true));
    }
    args
}
