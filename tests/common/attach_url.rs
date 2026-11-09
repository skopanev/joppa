//! requirement:file-upload over a real server: the test origin O for joppa_attach with url, and
//! the calls of the upload link tests. O is plain HTTP/1.1 on 127.0.0.1. Each connection gets
//! one reply and then closes.
#![allow(dead_code)]
use crate::record_world::World;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;
use std::time::Duration;

/// 50 MiB and 1 byte: one byte more than an attachment can have.
pub const OVER: usize = 50 * 1024 * 1024 + 1;

pub struct Origin {
    /// http://127.0.0.1:<port>, with no slash at the end.
    pub root: String,
    /// The 2048 bytes of /a.txt, different in each test process.
    pub a: Vec<u8>,
}

/// The one origin of this test process. It runs on its own threads.
pub fn origin() -> &'static Origin {
    static ORIGIN: OnceLock<Origin> = OnceLock::new();
    ORIGIN.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let seed = uuid::Uuid::new_v4().simple().to_string();
        let a: Vec<u8> = seed.bytes().cycle().take(2048).collect();
        let (r, b) = (root.clone(), a.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (r, b) = (r.clone(), b.clone());
                std::thread::spawn(move || {
                    let _ = reply(stream, &r, &b);
                });
            }
        });
        Origin { root, a }
    })
}

/// The variables that let the server under test reach O.
pub fn origin_env() -> Vec<(&'static str, String)> {
    vec![("JOPPA_ATTACH_URL_TEST_ORIGIN", origin().root.clone())]
}

fn head(status: &str, fields: &str) -> String {
    format!("HTTP/1.1 {status}\r\n{fields}Connection: close\r\n\r\n")
}

fn redirect(to: &str) -> String {
    head(
        "302 Found",
        &format!("Location: {to}\r\nContent-Length: 0\r\n"),
    )
}

/// Write `len` bytes of zeros. With `chunked`, each piece is one chunk of the chunked coding.
fn zeros(stream: &mut TcpStream, len: usize, chunked: bool) -> std::io::Result<()> {
    let piece = vec![0u8; 64 * 1024];
    let mut left = len;
    while left > 0 {
        let n = left.min(piece.len());
        if chunked {
            stream.write_all(format!("{n:x}\r\n").as_bytes())?;
        }
        stream.write_all(&piece[..n])?;
        if chunked {
            stream.write_all(b"\r\n")?;
        }
        left -= n;
    }
    if chunked {
        stream.write_all(b"0\r\n\r\n")?;
    }
    Ok(())
}

/// One reply of O. /hop/<n> redirects to /hop/<n-1>, and /hop/1 redirects to /a.txt.
fn reply(mut stream: TcpStream, root: &str, a: &[u8]) -> std::io::Result<()> {
    let mut request = Vec::new();
    let mut buf = [0u8; 4096];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let a_txt = head(
        "200 OK",
        &format!(
            "Content-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n",
            a.len()
        ),
    );
    let hop = |n: u32| match n {
        0 | 1 => format!("{root}/a.txt"),
        n => format!("{root}/hop/{}", n - 1),
    };
    match path.as_str() {
        "/a.txt" | "/slow" => {
            if path == "/slow" {
                std::thread::sleep(Duration::from_secs(31));
            }
            stream.write_all(a_txt.as_bytes())?;
            stream.write_all(a)
        }
        "/three-hops" => stream.write_all(redirect(&hop(3)).as_bytes()),
        "/redirect-loop" => stream.write_all(redirect(&hop(4)).as_bytes()),
        "/redirect-private" => stream.write_all(redirect("https://127.0.0.1:1/a").as_bytes()),
        "/redirect-http" => stream.write_all(redirect("http://example.com/a").as_bytes()),
        "/big-length" => {
            let fields = format!("Content-Type: text/plain\r\nContent-Length: {OVER}\r\n");
            stream.write_all(head("200 OK", &fields).as_bytes())?;
            zeros(&mut stream, OVER, false)
        }
        "/big-chunked" => {
            let fields = "Content-Type: text/plain\r\nTransfer-Encoding: chunked\r\n";
            stream.write_all(head("200 OK", fields).as_bytes())?;
            zeros(&mut stream, OVER, true)
        }
        p if p.starts_with("/hop/") => {
            let n = p["/hop/".len()..].parse().unwrap_or(0);
            stream.write_all(redirect(&hop(n)).as_bytes())
        }
        _ => stream
            .write_all(
                head(
                    "404 Not Found",
                    "Content-Type: text/plain\r\nContent-Length: 7\r\n",
                )
                .as_bytes(),
            )
            .and_then(|()| stream.write_all(b"missing")),
    }
}

/// The lowercase SHA-256 of `bytes`.
pub fn sha(bytes: &[u8]) -> String {
    joppa::digest_hex::sha256_hex(bytes)
}

/// A reply that must succeed.
pub fn ok(reply: Result<Value, String>) -> Result<Value> {
    reply.map_err(anyhow::Error::msg)
}

/// One joppa_attach call. A refusal gives Err with the exact error text.
pub async fn attach(w: &World, token: &str, args: Value) -> Result<Result<Value, String>> {
    w.mcp(token, "joppa_attach", args).await
}

/// None when `reply` is a refusal with `text`, else a line that tells the difference.
pub fn refusal(step: &str, reply: &Result<Value, String>, text: &str) -> Option<String> {
    match reply {
        Err(e) if e.contains(text) => None,
        other => Some(format!("{step}: must be refused with {text:?}: {other:?}")),
    }
}

/// The files of `target` from joppa_attachments for the member of `token`.
pub async fn files(w: &World, token: &str, target: &str) -> Result<Vec<Value>> {
    let reply = w.mcp(token, "joppa_attachments", json!({"target":target}));
    let reply = reply.await?.map_err(anyhow::Error::msg)?;
    reply["files"].as_array().cloned().context("files")
}

/// The upload link of one joppa_attach call of bot with upload true on `target`, with the
/// extra fields of `fields`. Gives the whole reply.
pub async fn link(w: &World, target: &str, fields: Value) -> Result<Value> {
    let mut args = json!({"target":target,"filename":"b.pdf","upload":true});
    for (k, v) in fields.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    let reply = attach(w, &w.bot, args).await?;
    let reply = reply.map_err(|e| anyhow::anyhow!("upload link on {target}: {e}"))?;
    ensure!(reply["upload_url"].is_string(), "no upload_url: {reply}");
    Ok(reply)
}

/// One PUT of `body` to `url` with no token and no cookie. Gives the HTTP code and the JSON
/// body, or the body as a string when it is no JSON.
pub async fn put(url: &str, body: Vec<u8>, content_type: &str) -> Result<(u16, Value)> {
    let reply = reqwest::Client::new()
        .put(url)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(body)
        .send()
        .await?;
    let code = reply.status().as_u16();
    let text = reply.text().await?;
    Ok((
        code,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    ))
}
