// Part 1 of tests/read_statements_postgres.rs, included there, so the test names stay unchanged.

/// One measured test at a time: pg_stat_statements counts the statements of the whole database.
static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The statements of one measured call: each query, lowercase on one line, with its calls.
struct Statements(Vec<(String, i64)>);

impl Statements {
    fn total(&self) -> i64 {
        self.0.iter().map(|s| s.1).sum()
    }
    /// The calls of the statements whose text holds `part`.
    fn naming(&self, part: &str) -> i64 {
        self.0
            .iter()
            .filter(|s| s.0.contains(part))
            .map(|s| s.1)
            .sum()
    }
    /// The calls of the statements whose text starts with `prefix`.
    fn starting(&self, prefix: &str) -> i64 {
        self.0
            .iter()
            .filter(|s| s.0.starts_with(prefix))
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

/// The addresses `ac:c000` ... of the first `n` criteria of the fixture, separated by commas.
fn addresses(n: usize) -> String {
    let list: Vec<String> = (0..n).map(|i| format!("ac:c{i:03}")).collect();
    list.join(",")
}

async fn batch(w: &record_world::World, name: &str, operations: Vec<Value>) -> Result<()> {
    let request_id = format!("statements-{name}-{}", uuid::Uuid::new_v4().simple());
    let args = json!({"request_id":request_id,"operations":operations});
    w.mcp(&w.bot, "joppa_batch", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// 100 criteria `ac:c000` ... `ac:c099` under requirement:r, each with a related edge to r, one
/// comment and one file. The first 25 have one test node each. With `measured`, the database
/// has pg_stat_statements.
async fn world(measured: bool) -> Result<record_world::World> {
    let w = record_world::world(&[]).await?;
    if measured {
        let conn = w.pool.get().await?;
        conn.batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
            .await?;
    }
    let each = |n: usize, op: &dyn Fn(String) -> Value| -> Vec<Value> {
        (0..n)
            .map(|i| json!({"command": op(format!("c{i:03}"))}))
            .collect()
    };
    let save = |id: String| {
        json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:r",
        "data":{"title":id.to_uppercase(),"owner":"owner"},
        "relations":{"related":{"edges":[{"to":"requirement:r"}]}},"reason":"Statement test"})
    };
    batch(&w, "criteria", each(100, &save)).await?;
    let note =
        |id: String| json!({"op":"note","target":format!("ac:{id}"),"id":"n1","text":"Seen"});
    batch(&w, "notes", each(100, &note)).await?;
    let file = json!({"target":"ac:a","filename":"e.txt","content":"Evidence"});
    let stored = w.mcp(&w.bot, "joppa_attach", file).await?;
    let stored = stored.map_err(anyhow::Error::msg)?["file"].clone();
    let attach = |id: String| {
        json!({"op":"attach","target":format!("ac:{id}"),"id":"f1","name":"e.txt",
        "content_type":"text/plain","size":stored["size"],"hash":stored["hash"]})
    };
    batch(&w, "files", each(100, &attach)).await?;
    let test = |id: String| {
        json!({"op":"save_object","kind":"test","revision":0,"parent":format!("ac:{id}"),
        "data":{"title":format!("tests/x.rs > {id}"),"owner":"owner"},"reason":"Bind"})
    };
    batch(&w, "tests", each(25, &test)).await?;
    Ok(w)
}

async fn reset(w: &record_world::World) -> Result<()> {
    w.pool.get().await?.query_one(
        "SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)",
        &[],
    ).await?;
    Ok(())
}

/// The statements since the last reset, without the reads of pg_stat_statements itself.
async fn statements(w: &record_world::World) -> Result<Statements> {
    let rows = w.pool.get().await?.query(
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

/// Wait until the server runs no statement for 300 ms. The indexer and the loops of the
/// server may run after the writes of the fixture.
async fn quiet(w: &record_world::World) -> Result<()> {
    for _ in 0..40 {
        reset(w).await?;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if statements(w).await?.total() == 0 {
            return Ok(());
        }
    }
    bail!("the server runs statements without a request")
}

/// The statements of one call of `tool` with `args`, after a warm-up call: of 3 tries, the try
/// with the fewest calls, so a statement of a server loop does not count. A refusal gives Err.
async fn measure(
    w: &record_world::World,
    tool: &str,
    args: Value,
) -> Result<Result<Statements, String>> {
    if let Err(refusal) = w.mcp(&w.bot, tool, args.clone()).await? {
        return Ok(Err(refusal));
    }
    let mut best: Option<Statements> = None;
    for _ in 0..3 {
        quiet(w).await?;
        w.mcp(&w.bot, tool, args.clone())
            .await?
            .map_err(anyhow::Error::msg)?;
        let found = statements(w).await?;
        if best.as_ref().is_none_or(|b| found.total() < b.total()) {
            best = Some(found);
        }
    }
    best.map(Ok)
        .ok_or_else(|| anyhow::anyhow!("no measurement"))
}
