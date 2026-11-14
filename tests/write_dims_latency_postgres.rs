//! A write to a large workspace with a filter dimension, a computed field, many recorded
//! results, notes, criterion boundaries and former test titles.
#[path = "common/dims_tree.rs"]
mod dims_tree;
use anyhow::{Result, ensure};
use dims_tree::{Pool, batch, node, record, tree};
use joppa::store;
use serde_json::{Value, json};
use std::time::Instant;

/// The time of one live write in ms.
async fn timed(pool: &Pool, ws: &str, command: Value) -> Result<f64> {
    let start = Instant::now();
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, "owner", &id, &command).await?;
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}

/// A save of `address` that changes `data`.
async fn save(pool: &Pool, ws: &str, address: &str, data: Value) -> Result<Value> {
    let scope = store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    let object = store::read_objects(pool, ws, "owner", &scope).await?;
    let (kind, id) = address.split_once(':').unwrap();
    Ok(
        json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],
        "parent":object["document"]["parent"],"data":data,"reason":"Clarify"}),
    )
}

/// The saves of each address of `addresses` that change `data`, in batches.
async fn save_all(
    pool: &Pool,
    ws: &str,
    addresses: &[String],
    data: impl Fn(&str) -> Value,
) -> Result<()> {
    let mut commands = vec![];
    for address in addresses {
        commands.push(save(pool, ws, address, data(address)).await?);
    }
    batch(pool, ws, commands).await
}

// ac:0bpoijs4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_write_on_a_large_workspace_with_a_dimension_takes_at_most_100_ms() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    // 32 domains, 160 capabilities, 960 requirements, 9600 criteria and 9600 test nodes.
    let (ws, tests) = tree(&pool, 32, 10).await?;
    record(&pool, &ws, &tests, 10).await?;
    // 4000 criteria have a note with a long text.
    let text = "The note keeps the evidence of the check. ".repeat(30);
    let notes: Vec<Value> = (0..4000)
        .map(|n| {
            let (d, c, r, a) = (n / 300, n / 60 % 5, n / 10 % 6, n % 10);
            json!({"op":"note","target":format!("ac:d{d}c{c}r{r}a{a}"),"id":format!("n{n}"),"text":text})
        })
        .collect();
    batch(&pool, &ws, notes).await?;
    // Each requirement changes once, so each criterion below it gets a boundary.
    let requirements: Vec<String> = (0..960)
        .map(|n| format!("requirement:d{}c{}r{}", n / 30, n / 6 % 5, n % 6))
        .collect();
    save_all(
        &pool,
        &ws,
        &requirements,
        |_| json!({"body":"The changed rule"}),
    )
    .await?;
    // 4000 test nodes get a new title, so each keeps its former title.
    let renamed: Vec<String> = tests
        .iter()
        .take(4000)
        .map(|t| format!("test:{}t", &t[15..]))
        .collect();
    save_all(
        &pool,
        &ws,
        &renamed,
        |a| json!({"title":format!("tests/big.rs > {} again", &a[5..])}),
    )
    .await?;
    // The first write of a server process loads the workspace into memory.
    let warm = save(&pool, &ws, "ac:d0c0r0a0", json!({"body":"Warm"})).await?;
    timed(&pool, &ws, warm).await?;
    let mut times = vec![];
    for i in 0..5 {
        let ac = format!("d1c2r3a{i}");
        let body = format!("The new rule {i}");
        let changed = save(&pool, &ws, &format!("ac:{ac}"), json!({"body":body})).await?;
        times.push(timed(&pool, &ws, changed).await?);
        let new = node("test", &format!("{ac}x"), Some(format!("ac:{ac}")));
        times.push(timed(&pool, &ws, new).await?);
        let note = json!({"op":"note","target":format!("ac:{ac}"),"id":"seen","text":"Seen"});
        times.push(timed(&pool, &ws, note).await?);
    }
    println!("writes ms {times:?}");
    let worst = times.iter().cloned().fold(0.0, f64::max);
    ensure!(worst <= 100.0, "a write took {worst} ms: {times:?}");
    Ok(())
}
