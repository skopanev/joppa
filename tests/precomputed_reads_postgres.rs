//! A read of the root domains uses the stored rollups: it computes no status, no rollup and
//! no wait, and the 19th of 20 sorted read times is at most 50 ms.
use anyhow::{Result, ensure};
use joppa::{probe, store};
use serde_json::{Value, json};
use std::time::Instant;

const DOMAINS: usize = 5;
const PER_DOMAIN: usize = 40;
const REPORTS: usize = 50;

fn test_name(domain: usize, n: usize) -> String {
    format!("tests/rollup.rs > d{domain}_c{n}")
}

/// The fixture outcome of each test: even numbers pass, odd numbers fail.
fn passes(n: usize) -> bool {
    n.is_multiple_of(2)
}

async fn batch(pool: &deadpool_postgres::Pool, ws: &str, commands: Vec<Value>) -> Result<()> {
    for chunk in commands.chunks(100) {
        let operations: Vec<store::BatchOperation> = chunk
            .iter()
            .map(|command| store::BatchOperation {
                reference: None,
                command: command.clone(),
            })
            .collect();
        store::execute_remote_batch(
            pool,
            ws,
            "owner",
            &uuid::Uuid::new_v4().to_string(),
            &operations,
        )
        .await?;
    }
    Ok(())
}

fn counts() -> [u64; 3] {
    [&probe::STATUS, &probe::ROLLUP, &probe::WAITS].map(probe::read)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn root_reads_use_stored_rollups_and_stay_under_50_ms() -> Result<()> {
    // ac:precomputed-status-reads-ac1
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
    let ws = format!("rollup-reads-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Rollup reads"}),
    )
    .await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    batch(&pool, &ws, vec![json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "domain":{"title":"Domains","role":"group","status":"children","root":true,"parents":[]},
        "requirement":{"title":"Requirements","role":"requirement","status":"children","parents":["domain"]},
        "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]}
    }})]).await?;
    // 5 domains, 2 requirements in each, 20 criteria in each requirement: 200 criteria.
    let mut commands = vec![];
    for d in 0..DOMAINS {
        commands.push(json!({"op":"save_object","kind":"domain","id":format!("d{d}"),"revision":0,"parent":null,"data":{"title":format!("Domain {d}"),"owner":"owner"},"reason":"Create"}));
        for r in 0..2 {
            commands.push(json!({"op":"save_object","kind":"requirement","id":format!("d{d}r{r}"),"revision":0,"parent":format!("domain:d{d}"),"data":{"title":format!("Rule {d}.{r}"),"owner":"owner","body":"The rule"},"reason":"Create"}));
        }
        for n in 0..PER_DOMAIN {
            let id = format!("d{d}c{n}");
            commands.push(json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":format!("requirement:d{d}r{}", n % 2),"data":{"title":format!("Criterion {d}.{n}"),"owner":"owner"},"reason":"Create"}));
            commands.push(json!({"op":"set_ac_tests","target":format!("ac:{id}"),"tests":[test_name(d, n)],"reason":"Bind"}));
        }
    }
    batch(&pool, &ws, commands).await?;
    // 50 records of 4 tests each give every one of the 200 tests its fixture outcome.
    let all: Vec<(usize, usize)> = (0..DOMAINS)
        .flat_map(|d| (0..PER_DOMAIN).map(move |n| (d, n)))
        .collect();
    for (k, part) in all.chunks(all.len() / REPORTS).enumerate() {
        let results: Vec<Value> = part
            .iter()
            .map(|(d, n)| json!({"test":test_name(*d, *n),"status":if passes(*n) {"pass"} else {"fail"}}))
            .collect();
        store::record_results(
            &pool,
            &ws,
            "owner",
            &json!({"commit":format!("{:07x}", 0xa000000 + k),"results":results}),
        )
        .await?;
    }
    // A domain counts its requirements. Requirement r0 holds the even criteria, which all
    // pass; r1 holds the odd criteria, which all fail.
    let expected = json!({"passed": 1, "failed": 1, "total": 2});
    let scope = store::ObjectScope {
        kind: Some("domain"),
        limit: 50,
        ..Default::default()
    };
    store::read_objects(&pool, &ws, "owner", &scope).await?;
    let before = counts();
    let mut times = vec![];
    for _ in 0..20 {
        let start = Instant::now();
        let page = store::read_objects(&pool, &ws, "owner", &scope).await?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        let items = page["items"].as_array().cloned().unwrap_or_default();
        ensure!(items.len() == DOMAINS, "{page}");
        for item in &items {
            ensure!(item["verification_summary"] == expected, "{item}");
        }
    }
    ensure!(
        counts() == before,
        "the reads computed {:?} -> {:?}",
        before,
        counts()
    );
    times.sort_by(f64::total_cmp);
    println!("sorted read ms: {times:?}");
    ensure!(
        times[18] <= 50.0,
        "the 19th sorted read took {} ms",
        times[18]
    );
    Ok(())
}
