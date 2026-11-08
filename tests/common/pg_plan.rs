//! Helpers for the tests that read query plans: the isolated test pool, a workspace with many
//! nodes, vectors for the search rows, the SQL text of a statement in a source file, and the
//! nodes of an `EXPLAIN (FORMAT JSON)` plan.
#![allow(dead_code)]
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};

pub type Pool = deadpool_postgres::Pool;

/// The pool of the isolated test database, with the migrations applied.
pub async fn db() -> Result<Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
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
    Ok(pool)
}

/// A new workspace of the owner, with the agent "bot" as a member. Its ID is its name.
pub async fn workspace(pool: &Pool, prefix: &str) -> Result<String> {
    let ws = format!("{prefix}-{}", uuid::Uuid::new_v4().simple());
    let op = json!({"op":"workspace","title":prefix});
    store::execute_input(pool, &ws, "owner", "workspace", &op).await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    Ok(ws)
}

/// Run the commands as server writes, 100 in each batch.
pub async fn batch(pool: &Pool, ws: &str, commands: &[Value]) -> Result<()> {
    for chunk in commands.chunks(100) {
        let operations: Vec<store::BatchOperation> = chunk
            .iter()
            .map(|command| store::BatchOperation {
                reference: None,
                command: command.clone(),
            })
            .collect();
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_batch(pool, ws, "owner", &id, &operations).await?;
    }
    Ok(())
}

/// Save `count` root domains d0, d1, ... with a body each, and `notes` inbox notes.
pub async fn many_nodes(pool: &Pool, ws: &str, count: usize, notes: usize) -> Result<()> {
    let mut commands: Vec<Value> = (0..count)
        .map(|i| {
            json!({"op":"save_object","kind":"domain","id":format!("d{i}"),"revision":0,"parent":null,
                "data":{"title":format!("Domain {i}"),"owner":"owner","body":format!("Body of domain {i}")},
                "reason":"Create"})
        })
        .collect();
    commands.extend((0..notes).map(|i| {
        json!({"op":"save_object","kind":"note","id":format!("n{i}"),"revision":0,"parent":null,
            "data":{"title":format!("Note {i}"),"owner":"owner","body":format!("Text of note {i}")},
            "reason":"Create"})
    }));
    batch(pool, ws, &commands).await
}

/// Give each search row of the workspace a random vector of the current profile.
pub async fn vectors(pool: &Pool, ws: &str) -> Result<()> {
    let conn = pool.get().await?;
    for table in ["search_documents", "search_revisions"] {
        // The `local_id` term makes the subquery run once for each row, so each row gets
        // its own vector.
        let sql = format!(
            "UPDATE joppa.{table} t SET embedding=(SELECT array_agg(random()::real + 0.001)::real[]::vector FROM generate_series(1,1024) g WHERE t.local_id IS NOT NULL),embedding_profile=$2 WHERE workspace=$1"
        );
        conn.execute(&sql, &[&ws, &joppa::embeddings::PROFILE])
            .await?;
    }
    Ok(())
}

/// The statistics that autovacuum keeps on a live server.
pub async fn analyze(pool: &Pool) -> Result<()> {
    pool.get().await?.batch_execute("ANALYZE joppa.objects, joppa.state_entries, joppa.board_cache, joppa.search_documents, joppa.search_revisions").await?;
    Ok(())
}

/// The text of the one string literal of `source` that holds `marker`. A test that reads the
/// statement this way fails when the statement in the source changes its form, so the plan
/// that it checks is the plan of the code. Only the escapes `\"`, `\\` and `\n` occur in the
/// SQL literals of the source.
pub fn literal(source: &str, marker: &str) -> Result<String> {
    let found: Vec<usize> = source.match_indices(marker).map(|(i, _)| i).collect();
    ensure!(found.len() == 1, "{} literals hold {marker:?}", found.len());
    let bytes = source.as_bytes();
    let quote = |i: usize| bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\');
    let start = (0..found[0])
        .rev()
        .find(|&i| quote(i))
        .context("no opening quote")?;
    let end = (found[0]..bytes.len())
        .find(|&i| quote(i))
        .context("no closing quote")?;
    let text = &source[start + 1..end];
    Ok(text
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\\\", "\\"))
}

/// Each plan node of an `EXPLAIN (FORMAT JSON)` result, depth first.
pub fn nodes(explain: &Value) -> Vec<Value> {
    fn walk(node: &Value, out: &mut Vec<Value>) {
        out.push(node.clone());
        for child in node["Plans"].as_array().into_iter().flatten() {
            walk(child, out);
        }
    }
    let mut out = vec![];
    walk(&explain[0]["Plan"], &mut out);
    out
}

/// The plan of `sql` with these EXPLAIN options and parameters.
pub async fn explain(
    client: &impl tokio_postgres::GenericClient,
    options: &str,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Value> {
    let row = client
        .query_one(&format!("EXPLAIN ({options}) {sql}"), params)
        .await?;
    let text: Json = row.try_get(0)?;
    Ok(serde_json::from_str(&text.0)?)
}

/// The value of a `json` column: `EXPLAIN (FORMAT JSON)` gives its plan in one.
struct Json(String);

impl<'a> tokio_postgres::types::FromSql<'a> for Json {
    fn from_sql(
        _: &tokio_postgres::types::Type,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Json(std::str::from_utf8(raw)?.to_owned()))
    }
    fn accepts(ty: &tokio_postgres::types::Type) -> bool {
        [
            tokio_postgres::types::Type::JSON,
            tokio_postgres::types::Type::TEXT,
        ]
        .contains(ty)
    }
}

/// The nodes that read `table` by a sequential scan.
pub fn seq_scans(plan: &Value, table: &str) -> Vec<Value> {
    nodes(plan)
        .into_iter()
        .filter(|n| n["Node Type"] == "Seq Scan" && n["Relation Name"] == table)
        .collect()
}
