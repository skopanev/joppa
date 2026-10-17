//! The HTTP server: configuration and the router.
use super::*;

pub async fn serve(pool: Pool, host: &str, port: u16) -> Result<()> {
    let path = std::env::var("JOPPA_TOKENS_FILE").map_err(|_| {
        anyhow::anyhow!(
            "set JOPPA_TOKENS_FILE to a private JSON file mapping bearer tokens to actor IDs"
        )
    })?;
    let identities: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(path)?)?;
    ensure!(!identities.is_empty(), "identity file is empty");
    for (token, actor) in &identities {
        ensure!(token.len() >= 24, "tokens must have at least 24 characters");
        joppa::commands::identifier(actor)?;
    }
    let login = web_auth::Login::from_env()?;
    for identity in login.users.values() {
        ensure!(
            identities.values().any(|a| a == identity),
            "login identity needs a configured API actor"
        );
    }
    let oauth_origin = login.public_origin()?;
    let embedder = joppa::embeddings::Embedder::from_env()?;
    if let Some(embedder) = embedder.clone() {
        tokio::spawn(joppa::store::search::worker(pool.clone(), embedder));
    }
    let state = App {
        pool,
        identities: Arc::new(identities),
        login,
        oauth_origin,
        embedder,
    };
    let app = Router::new()
        .merge(oauth::routes())
        .merge(mcp::routes())
        .route("/", get(public_page))
        .route(
            "/social-preview.png",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/png")],
                    include_bytes!("social-preview.png").as_slice(),
                )
            }),
        )
        .route(
            "/favicon.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("favicon.svg"),
                )
            }),
        )
        .route(
            "/vendor/markdown.js",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                        (header::CACHE_CONTROL, "public, max-age=86400"),
                    ],
                    concat!(
                        include_str!("vendor/marked.umd.js"),
                        "\n",
                        include_str!("vendor/purify.min.js")
                    ),
                )
            }),
        )
        .route("/tree", get(|| async { Html(tree_page()) }))
        .route(
            "/guide",
            get(|| async {
                Html(
                    include_str!("guide.html")
                        .replace("/* BRAND_STYLE */", include_str!("brand.css")),
                )
            }),
        )
        .route("/health", get(health))
        .route("/auth/login", post(login_user))
        .route("/auth/poll", post(poll_login))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/settings", get(settings_page))
        .route("/api/workspaces", get(list).post(create_workspace))
        .route(
            "/api/email-grants",
            get(list_email_grants).post(grant_email),
        )
        .route("/api/access", get(list_owned_access))
        .route("/api/access/remove", post(remove_access))
        .route(
            "/api/domain-grants",
            get(list_domain_grants).post(grant_domain),
        )
        .route("/api/workspaces/{workspace}", get(read))
        .route("/api/workspaces/{workspace}/rename", post(rename_workspace))
        .route("/api/workspaces/{workspace}/priority", post(set_priority))
        .route("/api/workspaces/{workspace}/delete", post(delete_workspace))
        .route("/api/workspaces/{workspace}/board", get(board))
        .route("/api/workspaces/{workspace}/objects", get(objects))
        .route("/api/workspaces/{workspace}/tasks", get(tasks))
        .route("/api/workspaces/{workspace}/timeline", get(timeline))
        .route("/api/workspaces/{workspace}/tree", get(tree_schema))
        .route(
            "/api/workspaces/{workspace}/reports",
            post(upload_report).layer(axum::extract::DefaultBodyLimit::max(
                store::REPORT_MAX + 64 * 1024,
            )),
        )
        .route("/api/workspaces/{workspace}/tests", get(criterion_tests))
        .route(
            "/api/workspaces/{workspace}/agent-tokens",
            get(list_agent_tokens).post(create_agent_token),
        )
        .route(
            "/api/workspaces/{workspace}/agent-tokens/{id}/revoke",
            post(revoke_agent_token),
        )
        .route("/api/workspaces/{workspace}/commands", post(command))
        .route("/api/workspaces/{workspace}/order", post(reorder))
        .route("/api/workspaces/{workspace}/dependencies", post(dependency))
        // Умолчание axum — два мегабайта, и файл крупнее отлетал бы с
        // невнятным 413 раньше, чем дойдёт до нашей проверки предела.
        .route(
            "/api/workspaces/{workspace}/blobs",
            post(upload).layer(axum::extract::DefaultBodyLimit::max(store::ATTACHMENT_MAX)),
        )
        .route("/api/workspaces/{workspace}/blobs/{hash}", get(download))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            refuse_read_writes,
        ))
        .layer(axum::middleware::from_fn(no_store));
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("joppa: http://{host}:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
