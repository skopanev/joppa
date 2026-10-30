//! requirement:http-routes and requirement:mcp-tools over a real server: each listed route
//! answers its method, an unlisted path gets the router 404, and tools/list names exactly
//! the 15 MCP tools.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use reqwest::Method;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Every route of the server: the method and the path pattern.
const ROUTES: [(&str, &str); 54] = [
    ("GET", "/.well-known/oauth-protected-resource"),
    ("GET", "/.well-known/oauth-protected-resource/mcp"),
    ("GET", "/.well-known/oauth-authorization-server"),
    ("POST", "/oauth/register"),
    ("GET", "/oauth/authorize"),
    ("GET", "/oauth/details"),
    ("POST", "/oauth/approve"),
    ("POST", "/oauth/token"),
    ("POST", "/oauth/revoke"),
    ("POST", "/mcp"),
    ("GET", "/mcp"),
    ("GET", "/"),
    ("GET", "/social-preview.png"),
    ("GET", "/favicon.svg"),
    ("GET", "/vendor/markdown.js"),
    ("GET", "/board/{name}"),
    ("GET", "/tree"),
    ("GET", "/guide"),
    ("GET", "/health"),
    ("POST", "/auth/login"),
    ("POST", "/auth/poll"),
    ("GET", "/auth/google/callback"),
    ("POST", "/auth/logout"),
    ("GET", "/api/me"),
    ("GET", "/settings"),
    ("GET", "/api/workspaces"),
    ("POST", "/api/workspaces"),
    ("GET", "/api/email-grants"),
    ("POST", "/api/email-grants"),
    ("GET", "/api/access"),
    ("POST", "/api/access/remove"),
    ("GET", "/api/domain-grants"),
    ("POST", "/api/domain-grants"),
    ("GET", "/api/workspaces/{workspace}"),
    ("POST", "/api/workspaces/{workspace}/rename"),
    ("POST", "/api/workspaces/{workspace}/priority"),
    ("GET", "/api/workspaces/{workspace}/references"),
    ("GET", "/api/workspaces/{workspace}/board"),
    ("GET", "/api/workspaces/{workspace}/objects"),
    ("GET", "/api/workspaces/{workspace}/search"),
    ("GET", "/api/workspaces/{workspace}/tasks"),
    ("GET", "/api/workspaces/{workspace}/timeline"),
    ("GET", "/api/workspaces/{workspace}/history"),
    ("GET", "/api/workspaces/{workspace}/tree"),
    ("GET", "/api/workspaces/{workspace}/tests"),
    ("GET", "/api/workspaces/{workspace}/agent-tokens"),
    ("POST", "/api/workspaces/{workspace}/agent-tokens"),
    (
        "POST",
        "/api/workspaces/{workspace}/agent-tokens/{id}/revoke",
    ),
    ("POST", "/api/workspaces/{workspace}/commands"),
    ("POST", "/api/workspaces/{workspace}/order"),
    ("POST", "/api/workspaces/{workspace}/dependencies"),
    ("POST", "/api/workspaces/{workspace}/blobs"),
    ("GET", "/api/workspaces/{workspace}/blobs/{hash}"),
    // The delete goes last: it removes the workspace of the other routes.
    ("POST", "/api/workspaces/{workspace}/delete"),
];

const TOOLS: [&str; 15] = [
    "joppa_attach",
    "joppa_attachments",
    "joppa_batch",
    "joppa_command",
    "joppa_help",
    "joppa_history",
    "joppa_history_mark",
    "joppa_list",
    "joppa_objects",
    "joppa_record_results",
    "joppa_search",
    "joppa_tasks",
    "joppa_tests",
    "joppa_timeline",
    "joppa_tree",
];

/// The status, the Allow header and the body of one request with the owner token and no body.
async fn call(
    http: &reqwest::Client,
    w: &record_world::World,
    method: &str,
    path: &str,
) -> Result<(u16, Option<String>, String)> {
    let reply = http
        .request(
            Method::from_bytes(method.as_bytes())?,
            format!("{}{path}", w.root),
        )
        .bearer_auth(&w.owner)
        .send()
        .await?;
    let allow = reply
        .headers()
        .get(reqwest::header::ALLOW)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    Ok((reply.status().as_u16(), allow, reply.text().await?))
}

// http-routes-list
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn every_listed_route_answers_and_an_unlisted_path_is_404() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    // The router fallback: 404 and an empty body.
    for path in ["/api/no-such-route", "/no-such-page"] {
        let (status, _, body) = call(&http, &w, "GET", path).await?;
        ensure!(
            status == 404 && body.is_empty(),
            "{path}: {status} {body:?}"
        );
    }
    let upload: Value = http
        .post(format!("{}/api/workspaces/{}/blobs", w.root, w.ws))
        .bearer_auth(&w.owner)
        .body(format!("route list {}", uuid::Uuid::new_v4()))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let hash = upload["hash"].as_str().unwrap_or_default().to_owned();
    ensure!(hash.len() == 64, "{upload}");
    for (method, pattern) in ROUTES {
        let path = pattern
            .replace("{workspace}", &w.ws)
            .replace("{name}", "main.js")
            .replace("{hash}", &hash)
            .replace("{id}", "1");
        let (status, allow, body) = call(&http, &w, method, &path).await?;
        if (method, pattern) == ("GET", "/mcp") {
            ensure!(
                status == 405 && allow.as_deref() == Some("POST"),
                "GET /mcp: {status} {allow:?}"
            );
            continue;
        }
        ensure!(status != 405, "{method} {pattern}: 405");
        ensure!(
            !(status == 404 && body.is_empty()),
            "{method} {pattern}: the router has no such route"
        );
    }
    Ok(())
}

// mcp-tools-list
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_tools_list_names_exactly_15_tools() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.owner)
        .json(&json!({"jsonrpc":"2.0","id":7,"method":"tools/list"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(reply["jsonrpc"] == "2.0" && reply["id"] == 7, "{reply}");
    let names: BTreeSet<&str> = reply["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str())
        .collect();
    let expected: BTreeSet<&str> = TOOLS.into_iter().collect();
    ensure!(names == expected, "{names:?}");
    ensure!(
        reply["result"]["tools"].as_array().map(Vec::len) == Some(15),
        "a tool name repeats"
    );
    Ok(())
}
