//! requirement:member-rights over a real server: each human member has the rights that needed
//! the workspace owner before, an agent keeps its refusals, and no reply names a workspace
//! owner. bob (actor owner) created W. alex and another-member are human members of W, and
//! bot is an agent of W.
#[path = "common/hidden_lists.rs"]
mod hidden_lists;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use serde_json::{Value, json};

/// A credential of one HTTP request: a browser session cookie or a bearer token.
enum Who<'a> {
    Session(&'a str),
    Token(&'a str),
}

/// A new browser session of `actor`. Gives the cookie header value.
async fn session(w: &World, actor: &str) -> Result<String> {
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    joppa::sessions::create(&w.pool, &token, actor, None).await?;
    Ok(format!("joppa_session={token}"))
}

/// One HTTP request with the Origin of the server. A body makes it a POST. Gives the HTTP code
/// and the JSON body (null for a body that is no JSON).
async fn send(w: &World, who: &Who<'_>, path: &str, body: Option<Value>) -> Result<(u16, Value)> {
    let url = format!("{}{path}", w.root);
    let mut request = match &body {
        Some(body) => w.http.post(url).json(body),
        None => w.http.get(url),
    };
    request = request.header("origin", &w.root);
    request = match who {
        Who::Session(cookie) => request.header("cookie", *cookie),
        Who::Token(token) => request.bearer_auth(token),
    };
    let reply = request.send().await?;
    let code = reply.status().as_u16();
    let text = reply.text().await?;
    Ok((code, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

/// A request that must succeed. Gives the JSON body.
async fn accepted(w: &World, who: &Who<'_>, path: &str, body: Option<Value>) -> Result<Value> {
    let (code, reply) = send(w, who, path, body.clone()).await?;
    ensure!(code == 200, "{path} {body:?}: {code} {reply}");
    Ok(reply)
}

/// A request that must be refused. The refusal names no workspace owner. Gives the body.
async fn denied(w: &World, who: &Who<'_>, path: &str, body: Option<Value>) -> Result<Value> {
    let (code, reply) = send(w, who, path, body.clone()).await?;
    ensure!(code != 200, "{path} {body:?} was accepted: {reply}");
    no_owner_words(&reply.to_string())?;
    Ok(reply)
}

/// The text has no "workspace owner" and no "owner required".
fn no_owner_words(text: &str) -> Result<()> {
    ensure!(
        !text.contains("workspace owner") && !text.contains("owner required"),
        "the text names a workspace owner: {text}"
    );
    Ok(())
}

/// The item of W in GET /api/access for `who`. Its members have no owner and no admin key.
async fn access_item(w: &World, who: &Who<'_>) -> Result<Value> {
    let reply = accepted(w, who, "/api/access", None).await?;
    let items = reply["items"].as_array().context("items of /api/access")?;
    let item = items.iter().find(|i| i["id"] == w.ws.as_str());
    let item = item.cloned().with_context(|| format!("W in {reply}"))?;
    let members = item["members"].as_array().context("members")?;
    for member in members {
        ensure!(
            member.get("owner").is_none() && member.get("admin").is_none(),
            "{item}"
        );
    }
    Ok(item)
}

/// One command over HTTP with a bearer token. Gives the HTTP code and the body.
async fn http_command(w: &World, token: &str, command: Value) -> Result<(u16, Value)> {
    let path = format!("/api/workspaces/{}/commands", w.ws);
    let id = uuid::Uuid::new_v4().to_string();
    let body = json!({"request_id":id,"command":command});
    send(w, &Who::Token(token), &path, Some(body)).await
}

/// The human members of W in the full read, as a map of ID to role.
async fn humans(w: &World) -> Result<Value> {
    let full = joppa::store::read(&w.pool, &w.ws, "alex", &joppa::store::Scope::full()).await?;
    let members = full["index"]["members"]
        .as_object()
        .with_context(|| format!("members: {full}"))?;
    Ok(Value::Object(
        members
            .iter()
            .filter(|(_, role)| *role == "human")
            .map(|(id, role)| (id.clone(), role.clone()))
            .collect(),
    ))
}

include!("member_rights_http/part01.rs");
include!("member_rights_http/part02.rs");
