//! requirement:search-index-storage over a real server and a test provider: the vector and
//! word columns of the search tables, the word forms of the three text configurations, and
//! the reciprocal rank fusion with the constant 60. The tests run one at a time, because
//! each server indexes every pending row of the shared database.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn db() -> Result<(deadpool_postgres::Pool, String)> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok((pool, url))
}

/// A workspace with one domain for each title, written through the store.
async fn workspace(pool: &deadpool_postgres::Pool, titles: &[(&str, &str)]) -> Result<String> {
    let ws = format!("storage-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Search storage"}),
    )
    .await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    for (id, title) in titles {
        store::execute_input(pool, &ws, "owner", &uuid::Uuid::new_v4().to_string(), &json!({"op":"save_object","kind":"domain","id":id,"revision":0,"parent":null,"data":{"title":title,"owner":"owner"},"reason":"Search storage test text"})).await?;
    }
    Ok(ws)
}

/// Wait until every search row of the workspace has a vector.
async fn settled(pool: &deadpool_postgres::Pool, ws: &str) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        let pending: i64 = pool.get().await?.query_one("SELECT (SELECT count(*) FROM joppa.search_documents WHERE workspace=$1 AND embedding IS NULL)
            + (SELECT count(*) FROM joppa.search_revisions WHERE workspace=$1 AND embedding IS NULL)", &[&ws]).await?.get(0);
        if pending == 0 {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("rows still wait for a vector")
}

fn item<'a>(reply: &'a Value, id: &str) -> Option<&'a Value> {
    reply["items"]
        .as_array()?
        .iter()
        .find(|i| i["kind"] == "domain" && i["id"] == id)
}

// search-columns
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn search_rows_hold_a_1024_value_vector_and_weighted_words_in_three_languages() -> Result<()>
{
    let _serial = SERIAL.lock().await;
    let (pool, _) = db().await?;
    let conn = pool.get().await?;
    for table in ["joppa.search_documents", "joppa.search_revisions"] {
        let vector: String = conn
            .query_one(
                "SELECT format_type(atttypid,atttypmod) FROM pg_attribute WHERE attrelid=$1::text::regclass AND attname='embedding'",
                &[&table],
            )
            .await?
            .get(0);
        ensure!(vector == "vector(1024)", "{table}: {vector}");
        let expression: String = conn
            .query_one(
                "SELECT pg_get_expr(d.adbin,d.adrelid) FROM pg_attrdef d JOIN pg_attribute a ON a.attrelid=d.adrelid AND a.attnum=d.adnum WHERE d.adrelid=$1::text::regclass AND a.attname='terms'",
                &[&table],
            )
            .await?
            .get(0);
        // Drop the casts and the spaces that PostgreSQL adds when it prints the expression.
        let plain: String = expression
            .replace("::regconfig", "")
            .replace("::\"char\"", "")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        for config in ["simple", "english", "russian"] {
            for (column, weight) in [("title", "A"), ("content", "B")] {
                let part = format!("setweight(to_tsvector('{config}',{column}),'{weight}')");
                ensure!(plain.contains(&part), "{table} has no {part}: {expression}");
            }
        }
        let gin: i64 = conn
            .query_one(
                "SELECT count(*) FROM pg_indexes WHERE schemaname='joppa' AND tablename=$1 AND indexdef ILIKE '%USING gin (terms)%'",
                &[&table.trim_start_matches("joppa.")],
            )
            .await?
            .get(0);
        ensure!(gin == 1, "{table} has {gin} GIN indexes on terms");
    }
    Ok(())
}

// search-word-forms
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_russian_or_english_word_form_finds_its_stem() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = workspace(
        &pool,
        &[
            ("ru", "Сохранение черновика"),
            ("en", "Monthly invoices of the account"),
        ],
    )
    .await?;
    let (fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    settled(&pool, &ws).await?;
    // Without the provider, the reply holds only the word results.
    fake.down.store(true, Ordering::SeqCst);
    for (query, id) in [("сохранения", "ru"), ("invoice", "en")] {
        let reply = world::http_search(&server.root, "bot", &ws, &[("q", query)]).await?;
        ensure!(
            reply["semantic_status"] == "unavailable",
            "{query}: {reply}"
        );
        ensure!(
            item(&reply, id).is_some(),
            "{query} did not find {id}: {reply}"
        );
    }
    let reply = world::http_search(&server.root, "bot", &ws, &[("q", "зеркало")]).await?;
    ensure!(
        item(&reply, "ru").is_none() && item(&reply, "en").is_none(),
        "a word without a match found an object: {reply}"
    );
    Ok(())
}

// search-rrf
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn fusion_adds_1_over_60_plus_rank_from_each_list() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = workspace(&pool, &[("a", "Alpha plan"), ("b", "Beta plan")]).await?;
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    settled(&pool, &ws).await?;
    let reply = world::http_search(&server.root, "bot", &ws, &[("q", "alpha")]).await?;
    ensure!(reply["mode"] == "hybrid", "{reply}");
    let score = |id: &str| item(&reply, id).and_then(|i| i["score"].as_f64());
    // Alpha is first in the word list and in the vector list. Beta is second in the vector
    // list only.
    let (Some(alpha), Some(beta)) = (score("a"), score("b")) else {
        anyhow::bail!("an item is missing: {reply}")
    };
    ensure!((alpha - 2.0 / 61.0).abs() < 1e-9, "alpha score {alpha}");
    ensure!((beta - 1.0 / 62.0).abs() < 1e-9, "beta score {beta}");
    Ok(())
}
