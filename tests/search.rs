#[allow(dead_code)]
mod common;
use anyhow::Result;
use axum::{Json, Router, extract::State, routing::post};
use joppa::{embeddings::Embedder, store};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

#[derive(Clone, Default)]
struct Provider {
    calls: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    started: Arc<Notify>,
    resume: Arc<Notify>,
}
async fn embeddings(
    State(p): State<Provider>,
    Json(v): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    p.calls.fetch_add(1, Ordering::SeqCst);
    if p.pause.swap(false, Ordering::SeqCst) {
        p.started.notify_one();
        p.resume.notified().await;
    }
    if p.fail.load(Ordering::SeqCst) {
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
    }
    assert_eq!(v["model"], joppa::embeddings::MODEL);
    let data = v["input"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .rev()
        .map(|(index, s)| {
            let s = s.as_str().unwrap().to_lowercase();
            let mut vector = vec![0.0; 4096];
            let dimension = if s.contains("renewal") || s.contains("extend membership") {
                0
            } else if s.contains("invoice") {
                1
            } else {
                2
            };
            vector[dimension] = 1.0;
            json!({"index":index,"embedding":vector})
        })
        .collect::<Vec<_>>();
    (axum::http::StatusCode::OK, Json(json!({"data":data})))
}
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn current_search_is_scoped_fresh_rebuildable_and_hybrid_without_touching_history()
-> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let ws = format!("search-{}", uuid::Uuid::new_v4().simple());
    for (i, value) in common::setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{i}"), &value).await?;
    }
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "extra",
        &json!({"op":"domain","id":"unused","title":"Invoice handling","authority":"owner"}),
    )
    .await?;
    let mut revision = common::revision(1);
    revision["title"] = json!("Subscription renewal preserves access");
    revision["body"] = json!("Платежи за подписку принимаются на iOS.");
    store::execute_input(&pool, &ws, "owner", "wording", &revision).await?;
    let before = store::read(&pool, &ws, "owner").await?;
    let text = store::search::find(
        &pool,
        "agent",
        &ws,
        "renewal",
        Some("requirement"),
        10,
        None,
    )
    .await?;
    assert_eq!(text["mode"], "full_text");
    assert_eq!(text["semantic_status"], "not_configured");
    assert_eq!(text["items"][0]["id"], "r");
    assert_eq!(text["items"][0]["revision"], 2);
    let russian = store::search::find(
        &pool,
        "agent",
        &ws,
        "платежа",
        Some("requirement"),
        10,
        None,
    )
    .await?;
    assert_eq!(russian["items"][0]["id"], "r");
    let public = before["system_ids"]["ac"]["ios"].as_str().unwrap();
    assert_eq!(
        store::search::find(&pool, "agent", &ws, public, None, 10, None).await?["items"][0]["system_id"],
        public
    );
    assert!(
        store::search::find(&pool, "outsider", &ws, "renewal", None, 10, None)
            .await
            .is_err()
    );
    let foreign = format!("foreign-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &foreign,
        "other",
        "create",
        &json!({"op":"workspace","title":"Private"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &foreign,
        "other",
        "domain",
        &json!({"op":"domain","id":"secret","title":"Foreign-only needle","authority":"other"}),
    )
    .await?;
    assert!(
        store::search::find(&pool, "owner", &ws, "foreign-only", None, 10, None).await?["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A missing cache is reconstructed from the unchanged journal on an authorized search.
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.search_documents WHERE workspace=$1",
            &[&ws],
        )
        .await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.search_state WHERE workspace=$1", &[&ws])
        .await?;
    assert_eq!(
        store::search::find(
            &pool,
            "agent",
            &ws,
            "renewal",
            Some("requirement"),
            10,
            None
        )
        .await?["items"],
        text["items"]
    );
    assert_eq!(store::read(&pool, &ws, "owner").await?, before);
    let provider = Provider::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new()
        .route("/embeddings", post(embeddings))
        .with_state(provider.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let embedder = Embedder::new("test-only-key".into(), format!("http://{addr}/embeddings"))?;
    let denied =
        store::search::find(&pool, "outsider", &ws, "renewal", None, 10, Some(&embedder)).await;
    assert!(denied.is_err());
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        0,
        "denied queries must never reach the provider"
    );
    while store::search::index_batch(&pool, &embedder).await? > 0 {}
    let hybrid = store::search::find(
        &pool,
        "agent",
        &ws,
        "extend membership",
        Some("requirement"),
        10,
        Some(&embedder),
    )
    .await?;
    assert_eq!(hybrid["mode"], "hybrid");
    assert_eq!(hybrid["semantic_status"], "ready");
    assert_eq!(hybrid["items"][0]["id"], "r");
    assert_eq!(hybrid["embedded"], hybrid["indexed"]);
    provider.fail.store(true, Ordering::SeqCst);
    let fallback = store::search::find(
        &pool,
        "agent",
        &ws,
        "renewal",
        Some("requirement"),
        10,
        Some(&embedder),
    )
    .await?;
    assert_eq!(fallback["mode"], "full_text");
    assert_eq!(fallback["semantic_status"], "unavailable");
    assert_eq!(fallback["items"][0]["id"], "r");
    provider.fail.store(false, Ordering::SeqCst);
    // Invalidate one document, then edit it again while its old vector is in flight.
    store::execute_input(&pool,&ws,"owner","edit-1",&json!({"op":"edit_domain","id":"unused","title":"Invoice approval","authority":"owner","reason":"Clarify"})).await?;
    provider.pause.store(true, Ordering::SeqCst);
    let p = pool.clone();
    let e = embedder.clone();
    let indexing = tokio::spawn(async move { store::search::index_batch(&p, &e).await });
    provider.started.notified().await;
    store::execute_input(&pool,&ws,"owner","edit-2",&json!({"op":"edit_domain","id":"unused","title":"Archive this scope","authority":"owner","reason":"Changed again"})).await?;
    provider.resume.notify_one();
    indexing.await??;
    let row=pool.get().await?.query_one("SELECT embedding IS NULL FROM joppa.search_documents WHERE workspace=$1 AND kind='domain' AND local_id='unused'",&[&ws]).await?;
    assert!(
        row.get::<_, bool>(0),
        "late old vector must not label new wording"
    );
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "archive",
        &json!({"op":"set_domain_archived","id":"unused","archived":true,"reason":"Out of scope"}),
    )
    .await?;
    assert!(store::search::find(&pool,"agent",&ws,"archive",Some("domain"),10,None).await?["items"].as_array().unwrap().is_empty());
    // An omitted old AC disappears from search, but survives its historical revision.
    revision["expected_revision"] = json!(2);
    revision["acs"] = json!([]);
    store::execute_input(&pool, &ws, "owner", "remove-ac", &revision).await?;
    assert!(
        store::search::find(&pool, "agent", &ws, public, Some("ac"), 10, None).await?["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store::read_item(&pool, public, "agent").await?["selection"]["id"],
        "ios"
    );
    let new = format!("renamed-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(&pool,&ws,"owner","rename",&json!({"op":"rename_workspace","workspace_id":new,"reference_prefix":new,"title":"Renamed","reason":"Owner requested"})).await?;
    let found = store::search::find(
        &pool,
        "agent",
        &ws,
        "renewal",
        Some("requirement"),
        10,
        None,
    )
    .await?;
    assert_eq!(found["workspace"], new);
    assert!(
        found["items"][0]["system_id"]
            .as_str()
            .unwrap()
            .starts_with(&new)
    );
    let after = store::read(&pool, &ws, "owner").await?;
    store::migrate(&pool).await?;
    assert_eq!(store::read(&pool, &ws, "owner").await?, after);
    assert_eq!(
        store::search::find(
            &pool,
            "agent",
            &new,
            "renewal",
            Some("requirement"),
            10,
            None
        )
        .await?["items"],
        found["items"]
    );
    // The actual runtime grants support cache refresh without granting journal mutation.
    let role = format!("search_runtime_{}", uuid::Uuid::new_v4().simple());
    pool.get().await?.batch_execute(&format!("CREATE ROLE {role} NOLOGIN; GRANT USAGE ON SCHEMA joppa TO {role}; GRANT SELECT,INSERT ON ALL TABLES IN SCHEMA joppa TO {role}; GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA joppa TO {role}; GRANT UPDATE,DELETE ON joppa.search_documents,joppa.search_state TO {role};")).await?;
    let mut url = reqwest::Url::parse(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    url.query_pairs_mut()
        .append_pair("options", &format!("-c role={role}"));
    let runtime_url = url.as_str().replace("options=-c+role", "options=-c%20role");
    let runtime = store::pool(&runtime_url)?;
    store::execute_input(&runtime,&new,"owner","runtime-edit",&json!({"op":"edit_domain","id":"d","title":"Subscription access","authority":"owner","reason":"Clarify"})).await?;
    assert_eq!(
        store::search::find(
            &runtime,
            "agent",
            &new,
            "renewal",
            Some("requirement"),
            10,
            None
        )
        .await?["items"][0]["id"],
        "r"
    );
    let rc = runtime.get().await?;
    assert!(
        rc.execute("DELETE FROM joppa.events WHERE workspace=$1", &[&ws])
            .await
            .is_err()
    );
    assert!(
        rc.execute(
            "UPDATE joppa.command_receipts SET actor='intruder' WHERE workspace=$1",
            &[&ws]
        )
        .await
        .is_err()
    );
    drop(rc);
    runtime.close();
    pool.get()
        .await?
        .batch_execute(&format!("DROP OWNED BY {role}; DROP ROLE {role};"))
        .await?;
    server.abort();
    Ok(())
}
