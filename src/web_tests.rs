//! Tests of the web routes: shared fixtures and helpers.
use super::*;

fn browser(cookie: Option<&str>, origin: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, "joppa.test".parse().unwrap());
    headers.insert(header::ORIGIN, origin.parse().unwrap());
    if let Some(cookie) = cookie {
        headers.insert(header::COOKIE, cookie.parse().unwrap());
    }
    headers
}

/// A verified Google identity completes sign-in through the callback outcome and the poll.
async fn sign_in(app: &App, sub: &str, email: &str) -> AuthResult {
    sign_in_hd(app, sub, email, None).await
}

/// The same sign-in, with the Google hd claim when Some.
async fn sign_in_hd(app: &App, sub: &str, email: &str, hd: Option<&str>) -> AuthResult {
    let identity = serde_json::from_value::<crate::google_auth::Identity>(
        json!({"sub":sub,"email":email,"email_verified":true,"hd":hd,"nonce":"n"}),
    )
    .unwrap();
    let (id, _) = app.login.start(None).await.unwrap();
    app.login
        .verified_for_tests(&app.pool, &id, &identity)
        .await;
    let cookie = format!("joppa_login={id}");
    poll_login(
        State(app.clone()),
        browser(Some(&cookie), "http://joppa.test"),
    )
    .await
}

fn session(headers: &HeaderMap) -> String {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|v| v.to_str().ok()?.strip_prefix("joppa_session="))
        .and_then(|v| v.split(';').next())
        .map(|token| format!("joppa_session={token}"))
        .unwrap()
}

async fn count(app: &App, sql: &str, actor: &str) -> i64 {
    app.pool
        .get()
        .await
        .unwrap()
        .query_one(sql, &[&actor])
        .await
        .unwrap()
        .get(0)
}

const ORIGIN: &str = "http://joppa.test";

/// The app on the isolated test database, with one invited user and no rules file.
async fn test_app() -> Result<App> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(db.starts_with("joppa_test"), "test database required");
    store::migrate(&pool).await?;
    Ok(App {
        pool,
        identities: Default::default(),
        login: web_auth::Login::for_tests(),
        oauth_origin: None,
        embedder: None,
    })
}

async fn create(pool: &Pool, workspace: &str, owner: &str) -> Result<()> {
    let command = Command::Workspace {
        title: workspace.into(),
    };
    store::execute(pool, workspace, owner, "create", &command).await?;
    Ok(())
}

/// A browser session made without a sign-in.
async fn session_for(pool: &Pool, actor: &str) -> Result<String> {
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(pool, &token, actor, None).await?;
    Ok(format!("joppa_session={token}"))
}

async fn scalar(
    app: &App,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> i64 {
    let conn = app.pool.get().await.unwrap();
    conn.query_one(sql, params).await.unwrap().get(0)
}

/// The member list of W as its owner reads it, and every email and domain grant of W.
async fn members_and_grants(app: &App, w: &str, owner: &str) -> (Value, Vec<String>) {
    let Json(index) = read(
        State(app.clone()),
        Path(w.into()),
        browser(Some(owner), ORIGIN),
    )
    .await
    .unwrap();
    let grants = app
        .pool
        .get()
        .await
        .unwrap()
        .query(
            "SELECT email FROM joppa.email_grants WHERE workspace=$1
             UNION ALL SELECT 'domain '||domain FROM joppa.domain_grants WHERE workspace=$1 ORDER BY 1",
            &[&w],
        )
        .await
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();
    (index["index"]["members"].clone(), grants)
}

/// The workspace IDs that GET /api/workspaces lists for these headers.
async fn listed(app: &App, headers: HeaderMap) -> Vec<String> {
    let Json(items) = list(State(app.clone()), headers).await.unwrap();
    let ids = items.as_array().unwrap().iter();
    ids.map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

fn plain(reply: ApiResult) -> std::result::Result<Value, (StatusCode, Value)> {
    reply
        .map(|Json(value)| value)
        .map_err(|(status, Json(value))| (status, value))
}

async fn add_email(app: &App, cookie: &str, workspace: &str, email: &str) -> ApiResult {
    let body = EmailGrant {
        workspace: workspace.into(),
        email: email.into(),
    };
    grant_email(
        State(app.clone()),
        browser(Some(cookie), ORIGIN),
        Json(body),
    )
    .await
}

async fn add_domain(app: &App, cookie: &str, workspace: &str, domain: &str) -> ApiResult {
    let body = DomainGrant {
        workspace: workspace.into(),
        domain: domain.into(),
    };
    grant_domain(
        State(app.clone()),
        browser(Some(cookie), ORIGIN),
        Json(body),
    )
    .await
}

fn derived(sub: &str) -> String {
    format!("user-{}", joppa::digest_hex::sha256_hex(sub.as_bytes()))
}

const MEMBER_OF: &str =
    "SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2";
/// The Google hd of a company account in healthium.one.
const HEALTHIUM: Option<&str> = Some("healthium.one");
const SESSIONS_OF: &str = "SELECT count(*) FROM joppa.browser_sessions WHERE actor=$1";

#[test]
fn a_missing_status_projection_answers_503() {
    let (status, body) = failure(anyhow::Error::new(store::ProjectionNotReady));
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body.0["error"], "criterion status projection is not ready");
}

include!("web_tests_domain.rs");

include!("web_tests_email.rs");

include!("web_tests_email_more.rs");

include!("web_tests_grants.rs");

include!("web_tests_hd.rs");
include!("web_tests_hd_more.rs");
