//! A workspace with rows in every kind of table, and the row counts of one workspace.
#![allow(dead_code)]
#[path = "fake_spaces.rs"]
pub mod fake_spaces;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub async fn pool() -> Result<deadpool_postgres::Pool> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    ensure!(
        url.rsplit('/')
            .next()
            .is_some_and(|db| db.starts_with("joppa_test")),
        "test database required"
    );
    let pool = store::pool(&url)?;
    store::migrate(&pool).await?;
    fake_spaces::start().configure();
    Ok(pool)
}

pub async fn write(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    command: Value,
) -> Result<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, actor, &id, &command).await
}

/// A workspace with rows in every kind of table: journal, projections, receipts, grants,
/// tokens, test reports and an attachment. `shared` is attached in it too.
pub async fn workspace(
    pool: &deadpool_postgres::Pool,
    prefix: &str,
    shared: &str,
) -> Result<(String, String)> {
    let ws = format!("{prefix}-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "setup",
        &json!({"op":"workspace","title":prefix}),
    )
    .await?;
    write(
        pool,
        &ws,
        "owner",
        json!({"op":"member","id":"alex","role":"human"}),
    )
    .await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    write(
        pool,
        &ws,
        "owner",
        json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,
        "data":{"title":"Checks","owner":"owner"},"reason":"Fixture"}),
    )
    .await?;
    write(
        pool,
        &ws,
        "owner",
        json!({"op":"save_object","kind":"note","id":"n","revision":0,"parent":null,
        "data":{"title":"Source note","owner":"owner","body":"Input"},
        "links":[{"target":"domain:d","relation":"related"}],"reason":"Fixture"}),
    )
    .await?;
    store::mark_history(
        pool,
        &ws,
        "owner",
        &json!({"position":1,"reason":"Fixture mark"}),
    )
    .await?;
    let own = format!("only in {ws}");
    let storage = joppa::spaces::storage()?;
    let (hash, size) = storage.put(own.as_bytes()).await?;
    let (shared_hash, shared_size) = storage.put(shared.as_bytes()).await?;
    for (id, hash, size) in [("own", &hash, size), ("shared", &shared_hash, shared_size)] {
        write(
            pool,
            &ws,
            "owner",
            json!({"op":"attach","target":"note:n","id":id,"name":"file.txt",
            "content_type":"text/plain","size":size,"hash":hash}),
        )
        .await?;
    }
    store::grant_email(pool, &ws, "owner", "lena@example.org").await?;
    store::grant_domain(pool, &ws, "owner", "example.net").await?;
    let token = store::create_agent_token(pool, &ws, "owner", "helper", None).await?;
    store::revoke_agent_token(pool, &ws, "owner", token["id"].as_i64().context("id")?).await?;
    store::record_results(
        pool,
        &ws,
        "alex",
        &json!({"commit":"a011cf1",
        "results":[{"test":"tests/purge.rs > purge > passes","status":"pass"}]}),
    )
    .await?;
    Ok((ws, hash))
}

/// The row count of `ws` in each table with a workspace column, and of its child rows.
pub async fn rows(pool: &deadpool_postgres::Pool, ws: &str) -> Result<BTreeMap<String, i64>> {
    let conn = pool.get().await?;
    let mut out = BTreeMap::new();
    for row in conn.query("SELECT c.table_name::text FROM information_schema.columns c JOIN information_schema.tables t USING (table_schema, table_name) WHERE c.table_schema='joppa' AND c.column_name='workspace' AND t.table_type='BASE TABLE'", &[]).await? {
        let table: String = row.get(0);
        let sql = format!("SELECT count(*) FROM joppa.{table} WHERE workspace=$1");
        out.insert(table, conn.query_one(&sql, &[&ws]).await?.get(0));
    }
    for (name, sql) in [
        (
            "workspaces",
            "SELECT count(*) FROM joppa.workspaces WHERE id=$1",
        ),
        (
            "test_results",
            "SELECT count(*) FROM joppa.test_results r JOIN joppa.test_reports p ON p.id=r.report_id WHERE p.workspace=$1",
        ),
        (
            "test_report_receipts",
            "SELECT count(*) FROM joppa.test_report_receipts r JOIN joppa.test_reports p ON p.id=r.report_id WHERE p.workspace=$1",
        ),
        (
            "agent_token_revocations",
            "SELECT count(*) FROM joppa.agent_token_revocations r JOIN joppa.agent_tokens t ON t.id=r.token_id WHERE t.workspace=$1",
        ),
    ] {
        out.insert(name.into(), conn.query_one(sql, &[&ws]).await?.get(0));
    }
    Ok(out)
}

/// Spaces has the file `hash`.
pub fn blob(hash: &str) -> bool {
    fake_spaces::start().has(hash)
}

pub fn empty(rows: &BTreeMap<String, i64>) -> bool {
    rows.values().all(|n| *n == 0)
}
