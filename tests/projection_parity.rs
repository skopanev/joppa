//! Row parity between two builds. The same fixture and the same writes run on each build,
//! and after each transition the test writes every read to JOPPA_PARITY_OUT: each list page,
//! each object read, the test status of each criterion and the Task queue. Times and item
//! IDs are replaced by markers. A diff of the files of two builds shows any changed field.
//! It uses only the public store API, so it compiles on the builds before and after a change.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::json;

mod parity_support;
use parity_support::*;

#[tokio::test]
#[ignore = "requires an isolated joppa_test_* PostgreSQL and JOPPA_PARITY_OUT"]
async fn projection_parity_dump() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test_"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    use_workspace("parity");
    let mut out = std::fs::File::create(std::env::var("JOPPA_PARITY_OUT")?)?;
    let mut n = fixture(&pool).await?;
    let (addresses, criteria) = (ADDRESSES, CRITERIA);
    let mut step = 0;
    let mut mark = async |out: &mut std::fs::File, label: &str| -> Result<()> {
        step += 1;
        dump(
            &pool,
            out,
            &format!("{step:02} {label}"),
            &addresses,
            &criteria,
        )
        .await
    };
    mark(&mut out, "fixture").await?;
    let scope = store::ObjectScope {
        address: Some("ac:a1"),
        ..Default::default()
    };
    let process = store::read_objects(&pool, ws(), "owner", &scope).await?["process"].clone();
    run(&pool, &mut n, json!({"op":"assess","req":"requirement:r1","revision":process["revision"],"ac":"ac:a1","assessment":{"ready":true,"reason":"Clear","recipient":null,"decision":null}})).await?;
    let t1 = revision(&pool, "task:t1").await?;
    run(
        &pool,
        &mut n,
        json!({"op":"claim_task","id":"task:t1","revision":t1}),
    )
    .await?;
    let t1 = revision(&pool, "task:t1").await?;
    run(
        &pool,
        &mut n,
        json!({"op":"complete_task","id":"task:t1","revision":t1,"evidence":"Done"}),
    )
    .await?;
    mark(&mut out, "task done").await?;
    upload(
        &pool,
        "c1c1c1c",
        1,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "fail"),
            ("z1", "pass"),
        ],
    )
    .await?;
    mark(&mut out, "full report").await?;
    upload(
        &pool,
        "c2c2c2c",
        2,
        true,
        &[
            ("x1", "pass"),
            ("x1", "fail"),
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "pass"),
            ("z1", "pass"),
        ],
    )
    .await?;
    mark(&mut out, "one failing case of three").await?;
    upload(
        &pool,
        "c3c3c3c",
        3,
        false,
        &[("x2", "skipped"), ("y1", "pass")],
    )
    .await?;
    mark(&mut out, "partial report").await?;
    upload(
        &pool,
        "c4c4c4c",
        4,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "pass"),
        ],
    )
    .await?;
    mark(&mut out, "missing test").await?;
    upload(
        &pool,
        "c5c5c5c",
        5,
        true,
        &[
            ("x1", "pass"),
            ("x2", "skipped"),
            ("x3", "todo"),
            ("y1", "pass"),
            ("z1", "pass"),
        ],
    )
    .await?;
    mark(&mut out, "skipped and todo").await?;
    upload(
        &pool,
        "c6c6c6c",
        6,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "pass"),
            ("z1", "pass"),
        ],
    )
    .await?;
    upload(
        &pool,
        "c6c6c6c",
        6,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "fail"),
            ("z1", "pass"),
        ],
    )
    .await?;
    mark(&mut out, "same commit red wins").await?;
    upload(
        &pool,
        "c6c6c6c",
        6,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "pass"),
            ("z1", "pass"),
        ],
    )
    .await?;
    mark(&mut out, "repeat receipt").await?;
    run(&pool, &mut n, json!({"op":"set_ac_tests","target":"ac:a2","tests":list(&["y1"]),"reason":"Save the same list"})).await?;
    mark(&mut out, "identical list save").await?;
    run(
        &pool,
        &mut n,
        json!({"op":"set_ac_tests","target":"ac:a3","tests":[],"reason":"Clear the list"}),
    )
    .await?;
    mark(&mut out, "empty list save").await?;
    let r1 = revision(&pool, "requirement:r1").await?;
    run(&pool, &mut n, json!({"op":"save_object","kind":"requirement","id":"r1","revision":r1,"parent":"capability:c","data":{"title":"First","owner":"owner","body":"Rule one, changed.","consumers":["user"]},"reason":"Change the rule"})).await?;
    mark(&mut out, "ancestor edit").await?;
    let r2 = revision(&pool, "requirement:r2").await?;
    run(&pool, &mut n, json!({"op":"save_object","kind":"requirement","id":"r2","revision":r2,"parent":"capability:c2","data":{"title":"Second","owner":"owner","body":"Rule two.","consumers":["user"]},"reason":"Move the rule"})).await?;
    mark(&mut out, "ancestor move").await?;
    let r1 = revision(&pool, "requirement:r1").await?;
    run(&pool, &mut n, json!({"op":"save_object","kind":"requirement","id":"r1","revision":r1,"parent":"capability:c","data":{"title":"First","owner":"owner","body":"Rule one, changed.","consumers":["user"]},"links":[{"target":"requirement:r2","relation":"depends_on","blocking":true}],"reason":"Order after r2"})).await?;
    mark(&mut out, "blocking link").await?;
    let a2 = revision(&pool, "ac:a2").await?;
    run(
        &pool,
        &mut n,
        json!({"op":"archive_object","target":"ac:a2","revision":a2,"reason":"Cut the criterion"}),
    )
    .await?;
    mark(&mut out, "archive").await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws()])
        .await?;
    store::backfill_board(&pool).await?;
    mark(&mut out, "restart backfill").await?;
    Ok(())
}
