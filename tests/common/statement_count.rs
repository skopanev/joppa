//! The SQL statements of one call to a real server, as pg_stat_statements counts them in the
//! test database (the setup of tests/read_latency.rs). One measured test runs at a time.
#![allow(dead_code)]
use anyhow::{Result, bail};

/// One measured test at a time: pg_stat_statements counts the statements of the whole database.
pub static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The statements of one measured call: each query, lowercase on one line, with its calls.
pub struct Statements(pub Vec<(String, i64)>);

impl Statements {
    pub fn total(&self) -> i64 {
        self.0.iter().map(|s| s.1).sum()
    }
    /// The calls of the statements whose text holds each of `parts`.
    pub fn naming(&self, parts: &[&str]) -> i64 {
        self.0
            .iter()
            .filter(|s| parts.iter().all(|p| s.0.contains(p)))
            .map(|s| s.1)
            .sum()
    }
}

impl std::fmt::Display for Statements {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} calls:", self.total())?;
        for (query, calls) in &self.0 {
            writeln!(f, "{calls} x {query}")?;
        }
        Ok(())
    }
}

/// Add pg_stat_statements to the test database.
pub async fn install(pool: &deadpool_postgres::Pool) -> Result<()> {
    let conn = pool.get().await?;
    conn.batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .await?;
    Ok(())
}

async fn reset(pool: &deadpool_postgres::Pool) -> Result<()> {
    pool.get().await?.query_one(
        "SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)",
        &[],
    ).await?;
    Ok(())
}

/// The statements since the last reset, without the reads of pg_stat_statements itself.
async fn statements(pool: &deadpool_postgres::Pool) -> Result<Statements> {
    let rows = pool.get().await?.query(
        "SELECT query,calls FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND userid=(SELECT usesysid FROM pg_user WHERE usename=current_user)",
        &[],
    ).await?;
    let mut list: Vec<(String, i64)> = rows
        .iter()
        .map(|r| {
            let query: String = r.get(0);
            let words: Vec<&str> = query.split_whitespace().collect();
            (words.join(" ").to_ascii_lowercase(), r.get(1))
        })
        .filter(|(query, _)| !query.contains("pg_stat_statements"))
        .collect();
    list.sort();
    Ok(Statements(list))
}

/// Wait until the server runs no statement for 300 ms.
async fn quiet(pool: &deadpool_postgres::Pool) -> Result<()> {
    for _ in 0..40 {
        reset(pool).await?;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if statements(pool).await?.total() == 0 {
            return Ok(());
        }
    }
    bail!("the server runs statements without a request")
}

/// The statements of one `call`, after a warm-up call: of 3 tries, the try with the fewest
/// calls, so a statement of a server loop does not count.
pub async fn measure(
    pool: &deadpool_postgres::Pool,
    call: impl std::ops::AsyncFn() -> Result<()>,
) -> Result<Statements> {
    call().await?;
    let mut best: Option<Statements> = None;
    for _ in 0..3 {
        quiet(pool).await?;
        call().await?;
        let found = statements(pool).await?;
        if best.as_ref().is_none_or(|b| found.total() < b.total()) {
            best = Some(found);
        }
    }
    best.ok_or_else(|| anyhow::anyhow!("no measurement"))
}
