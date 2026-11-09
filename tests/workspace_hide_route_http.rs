//! requirement:owner-manages-workspace over a real server: a browser session hides a workspace
//! for its own member with POST /api/workspaces/{workspace}/hide. The route checks the origin,
//! refuses a bearer token and keeps a workspace closed to a person who is not a member.
#[path = "common/hidden_lists.rs"]
mod hidden_lists;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use hidden_lists::web;
use record_world::World;
use serde_json::{Value, json};

/// A browser session of `actor`: the value of the joppa_session cookie.
async fn session(w: &World, actor: &str) -> Result<String> {
    let cookie = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    joppa::sessions::create(&w.pool, &cookie, actor, None).await?;
    Ok(cookie)
}

/// The credential of one hide request.
enum Who<'a> {
    Cookie(&'a str),
    Bearer(&'a str),
}

/// POST /api/workspaces/W/hide with `body`: the HTTP code and the reply body.
async fn hide(w: &World, who: Who<'_>, origin: Option<&str>, body: Value) -> Result<(u16, Value)> {
    let url = format!("{}/api/workspaces/{}/hide", w.root, w.ws);
    let mut request = w.http.post(url).json(&body);
    request = match who {
        Who::Cookie(cookie) => request.header("Cookie", format!("joppa_session={cookie}")),
        Who::Bearer(token) => request.bearer_auth(token),
    };
    if let Some(origin) = origin {
        request = request.header("Origin", origin);
    }
    let reply = request.send().await?;
    let code = reply.status().as_u16();
    let text = reply.text().await?;
    Ok((
        code,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    ))
}

/// GET /api/workspaces of alex omits W (`alex_hides`), and that of O lists W.
async fn lists(w: &World, alex_hides: bool, step: &str) -> Result<()> {
    let alex = web(w, &w.alex, "/api/workspaces").await?;
    ensure!(
        alex.contains(&w.ws) != alex_hides,
        "{step}: GET /api/workspaces of alex {alex:?}"
    );
    let owner = web(w, &w.owner, "/api/workspaces").await?;
    ensure!(
        owner.contains(&w.ws),
        "{step}: GET /api/workspaces of O {owner:?}"
    );
    Ok(())
}

// ac:k0mrgse5
// test:zd3fuozh
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_browser_session_hides_a_workspace_for_its_member_through_the_hide_route() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let alex = session(&w, "alex").await?;
    let _owner = session(&w, "owner").await?;
    // eve has a session and no membership of W.
    let eve = session(&w, "outsider").await?;
    let root = w.root.clone();
    let on = json!({"hidden": true});
    lists(&w, false, "before").await?;
    let reply = hide(&w, Who::Cookie(&alex), Some(&root), on.clone()).await?;
    ensure!(
        reply == (200, json!({"id": w.ws, "hidden": true})),
        "alex: {reply:?}"
    );
    lists(&w, true, "alex hides").await?;
    // A refused request of alex changes no list: hidden: false would show W again.
    let foreign = Some("http://evil.example");
    for (origin, error) in [
        (foreign, "cross-origin request denied"),
        (None, "origin required"),
    ] {
        for hidden in [true, false] {
            let body = json!({ "hidden": hidden });
            let reply = hide(&w, Who::Cookie(&alex), origin, body).await?;
            ensure!(
                reply == (403, json!({ "error": error })),
                "alex with origin {origin:?} and hidden {hidden}: {reply:?}"
            );
            lists(&w, true, &format!("origin {origin:?}, hidden {hidden}")).await?;
        }
    }
    let reply = hide(&w, Who::Bearer(&w.bot), Some(&root), on.clone()).await?;
    ensure!(
        reply
            == (
                403,
                json!({"error": "Sign in with the browser to hide a workspace"})
            ),
        "bot: {reply:?}"
    );
    lists(&w, true, "bot").await?;
    let bot = hidden_lists::listed(&w, &w.bot).await?;
    ensure!(bot.get("hidden").is_none(), "joppa_list of bot: {bot}");
    let reply = hide(&w, Who::Cookie(&eve), Some(&root), on).await?;
    ensure!(
        reply == (404, json!({"error": "Workspace not found"})),
        "eve: {reply:?}"
    );
    lists(&w, true, "eve").await?;
    Ok(())
}
