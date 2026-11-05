//! requirement:precomputed-status-reads: a write recomputes the status of the nodes that it
//! reaches only, and after each write the stored statuses equal a full rebuild.
use anyhow::{Result, ensure};
use joppa::{probe, store};
use serde_json::{Value, json};

type Rows = Vec<(String, Option<String>, Option<String>)>;

/// The stored status of each object of the workspace.
async fn rows(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Rows> {
    let sql = "SELECT address,test_status::text,rollup::text FROM joppa.objects WHERE workspace=$1 ORDER BY address";
    let rows = pool.get().await?.query(sql, &[&ws]).await?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}

/// One write: it must take the partial path unless `full`, and its result must equal a full
/// rebuild of the same rows.
async fn step(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    label: &str,
    full: bool,
    write: impl std::future::Future<Output = Result<Value>>,
) -> Result<()> {
    let (delta, rebuild) = (probe::read(&probe::DELTA), probe::read(&probe::REBUILD));
    write.await.map_err(|e| anyhow::anyhow!("{label}: {e:#}"))?;
    let partial = probe::read(&probe::REBUILD) == rebuild;
    ensure!(partial != full, "{label}: partial {partial}");
    ensure!(
        full || probe::read(&probe::DELTA) == delta + 1,
        "{label}: no partial update"
    );
    let written = rows(pool, ws).await?;
    store::catch_up_statuses(pool, true).await?;
    let rebuilt = rows(pool, ws).await?;
    for (a, b) in written.iter().zip(&rebuilt) {
        ensure!(a == b, "{label}: partial {a:?}\nfull {b:?}");
    }
    ensure!(written.len() == rebuilt.len(), "{label}: row count");
    Ok(())
}

// ac:nslp4ws9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_write_recomputes_the_reached_nodes_and_equals_a_full_rebuild() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let ws = format!("delta-{}", uuid::Uuid::new_v4().simple());
    let op = serde_json::from_value(json!({"op":"workspace","title":"Delta"}))?;
    store::execute(&pool, &ws, "owner", "setup-0", &op).await?;
    let pool = &pool;
    let ws = ws.as_str();
    let write = |command: Value| async move {
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_input(pool, ws, "owner", &id, &command).await
    };
    let record = |commit: &str, results: Value| {
        let args = json!({"workspace":ws,"commit":commit,"results":results});
        async move { store::record_results(pool, ws, "owner", &args).await }
    };
    let save = |kind: &str, id: &str, parent: Value, links: Value| {
        json!({"op":"save_object","kind":kind,"id":id,"revision":0,
        "parent":parent,"data":{"title":id,"owner":"owner","body":"The rule"},"links":links,"reason":"Create"})
    };
    let test = |id: &str, parent: &str| {
        json!({"op":"save_object","kind":"test","id":id,"revision":0,
        "parent":parent,"data":{"title":format!("tests/x.rs > {id}"),"owner":"owner"},"reason":"Bind"})
    };
    let pass =
        |id: &str, status: &str| json!({"test":format!("tests/x.rs > {id}"),"status":status});
    let tree = store::tree_schema(pool, ws, "owner", None).await?;
    step(pool, ws, "schema", true, write(json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400},
        "area":{"title":"Areas","role":"document","status":"links","root":true,"parents":[]}}}))).await?;
    let none = json!([]);
    for (id, kind, parent) in [
        ("r", "requirement", Value::Null),
        ("r2", "requirement", Value::Null),
        ("x", "area", Value::Null),
        ("a", "ac", json!("requirement:r")),
        ("b", "ac", json!("requirement:r")),
    ] {
        step(
            pool,
            ws,
            id,
            false,
            write(save(kind, id, parent, none.clone())),
        )
        .await?;
    }
    for (id, parent) in [("t1", "ac:a"), ("t2", "ac:a"), ("t3", "ac:b")] {
        step(pool, ws, id, false, write(test(id, parent))).await?;
    }
    step(
        pool,
        ws,
        "one pass",
        false,
        record(
            "1111111111111111111111111111111111111111",
            json!([pass("t1", "pass")]),
        ),
    )
    .await?;
    step(
        pool,
        ws,
        "two passes",
        false,
        record(
            "2222222222222222222222222222222222222222",
            json!([pass("t1", "pass"), pass("t2", "pass")]),
        ),
    )
    .await?;
    step(
        pool,
        ws,
        "a failure",
        false,
        record(
            "3333333333333333333333333333333333333333",
            json!([pass("t3", "fail")]),
        ),
    )
    .await?;
    step(
        pool,
        ws,
        "a waiting mark",
        false,
        write(json!({"op":"assess","target":"ac:b","ready":false,"reason":"Which limit applies?"})),
    )
    .await?;
    let move_a = json!({"op":"save_object","kind":"ac","id":"a","revision":1,"parent":"requirement:r2",
        "data":{"title":"a","owner":"owner","body":"The rule"},"links":[{"target":"area:x","relation":"related"}],"reason":"Move"});
    step(pool, ws, "move and link", false, write(move_a)).await?;
    let depend = json!({"op":"save_object","kind":"ac","id":"b","revision":1,"parent":"requirement:r",
        "data":{"title":"b","owner":"owner","body":"The rule"},"links":[{"target":"ac:a","relation":"depends_on","blocking":true}],"reason":"Depend"});
    step(pool, ws, "a dependency", false, write(depend)).await?;
    step(
        pool,
        ws,
        "the target fails",
        false,
        record(
            "4444444444444444444444444444444444444444",
            json!([pass("t1", "fail")]),
        ),
    )
    .await?;
    step(
        pool,
        ws,
        "the target passes",
        false,
        record(
            "5555555555555555555555555555555555555555",
            json!([pass("t1", "pass")]),
        ),
    )
    .await?;
    step(
        pool,
        ws,
        "an archive",
        false,
        write(json!({"op":"archive_object","target":"test:t3","revision":1,"reason":"Not needed"})),
    )
    .await?;
    // Two areas that link to each other form a cycle: only the full rebuild orders it.
    let area = |id: &str, revision: u32, target: &str| {
        json!({"op":"save_object","kind":"area","id":id,"revision":revision,
        "parent":null,"data":{"title":id,"owner":"owner","body":"The rule"},"links":[{"target":target,"relation":"related"}],"reason":"Link"})
    };
    step(
        pool,
        ws,
        "a linked area",
        false,
        write(area("y", 0, "area:x")),
    )
    .await?;
    step(
        pool,
        ws,
        "a links cycle",
        true,
        write(area("x", 1, "area:y")),
    )
    .await?;
    Ok(())
}
