//! The synthetic fixture of the board baseline: the shape of joppa, scaled past 700 objects.
// Each test target that includes this module uses a different part of it.
#![allow(dead_code)]
use anyhow::Result;
use joppa::store;
use serde_json::{Value, json};

pub const SEED: u64 = 20_261_017;
pub const CAPS: usize = 25;
pub const REQS: usize = 76;
pub const ACS: usize = 202;
pub const TASKS: usize = 420;
pub const REPORTS: usize = 40;
pub const FILE: &str = "tests/synthetic_board.rs";

/// One write batch of at most 100 operations, as the server allows.
pub async fn write(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    step: &mut usize,
    ops: Vec<Value>,
) -> Result<()> {
    for chunk in ops.chunks(100) {
        let operations = chunk
            .iter()
            .map(|command| serde_json::from_value(json!({ "command": command })))
            .collect::<Result<Vec<store::BatchOperation>, _>>()?;
        store::execute_batch(pool, ws, "owner", &format!("baseline-{step}"), &operations).await?;
        *step += 1;
    }
    Ok(())
}

pub fn save(kind: &str, id: &str, parent: Option<String>, data: Value) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Build the synthetic fixture"})
}

/// Requirement links as in joppa: 9 blocking and 4 ordering-only references.
pub fn links(r: usize) -> Value {
    let to = |target: usize, blocking: bool| json!({"target":format!("requirement:r{target}"),"relation":"depends_on","blocking":blocking});
    match r {
        10 => json!([
            to(1, true),
            to(2, true),
            to(3, true),
            to(4, true),
            to(5, true)
        ]),
        20 => json!([to(6, true)]),
        30 => json!([to(7, true)]),
        40 => json!([to(8, true)]),
        50 => json!([
            to(9, true),
            to(11, false),
            to(12, false),
            to(13, false),
            to(14, false)
        ]),
        _ => json!([]),
    }
}

pub fn test_name(ac: usize, case: usize) -> String {
    format!("a{ac}_case{case}")
}

/// A fixed pseudo-random sequence, so the failures repeat between runs.
pub fn fails(report: usize, ac: usize, case: usize) -> bool {
    let x = SEED ^ ((report as u64) << 32) ^ ((ac as u64) << 8) ^ case as u64;
    x.wrapping_mul(6_364_136_223_846_793_005)
        .rotate_left(17)
        .is_multiple_of(97)
}

pub fn junit(report: usize) -> String {
    let mut cases = String::new();
    for ac in 0..ACS {
        for case in 0..2 {
            let name = test_name(ac, case);
            if fails(report, ac, case) {
                cases.push_str(&format!(
                    r#"<testcase name="{name}"><failure message="synthetic"/></testcase>"#
                ));
            } else {
                cases.push_str(&format!(r#"<testcase name="{name}"/>"#));
            }
        }
    }
    format!(r#"<testsuites><testsuite name="{FILE}">{cases}</testsuite></testsuites>"#)
}

pub async fn build(pool: &deadpool_postgres::Pool, ws: &str) -> Result<()> {
    store::execute_input(
        pool,
        ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Board baseline"}),
    )
    .await?;
    let mut step = 0;
    let owner = |title: String| json!({"title":title,"owner":"owner"});
    let mut ops = vec![save("consumer", "user", None, owner("User".into()))];
    ops.extend((0..5).map(|d| {
        save(
            "domain",
            &format!("d{d}"),
            None,
            owner(format!("Domain {d}")),
        )
    }));
    ops.extend((0..CAPS).map(|c| {
        save(
            "capability",
            &format!("c{c}"),
            Some(format!("domain:d{}", c % 5)),
            owner(format!("Capability {c}")),
        )
    }));
    write(pool, ws, &mut step, ops).await?;
    let reqs = (0..REQS)
        .map(|r| {
            let mut command = save("requirement", &format!("r{r}"), Some(format!("capability:c{}", r % CAPS)),
                json!({"title":format!("Requirement {r}"),"owner":"owner","body":"Synthetic rule.","consumers":["user"]}));
            command["links"] = links(r);
            command
        })
        .collect();
    write(pool, ws, &mut step, reqs).await?;
    let acs = (0..ACS)
        .map(|a| save("ac", &format!("a{a}"), Some(format!("requirement:r{}", a % REQS)),
            json!({"title":format!("Criterion {a}"),"owner":"owner",
                "description":"Given: a state.\nWhen: an action.\nThen: a result.\nEvidence: a check."})))
        .collect();
    write(pool, ws, &mut step, acs).await?;
    let tasks = (0..TASKS)
        .map(|t| save("task", &format!("t{t}"), Some(format!("ac:a{}", t % ACS)),
            json!({"title":format!("Task {t}"),"owner":"owner","contract":"Synthetic work","purpose":"implementation","projects":[]})))
        .collect();
    write(pool, ws, &mut step, tasks).await?;
    let lists = (0..ACS)
        .map(|a| json!({"op":"set_ac_tests","target":format!("ac:a{a}"),
            "tests":[format!("{FILE} > {}", test_name(a, 0)), format!("{FILE} > {}", test_name(a, 1))],"reason":"Bind the synthetic tests"}))
        .collect();
    write(pool, ws, &mut step, lists).await?;
    for report in 0..REPORTS {
        let stamp = json!({"commit":format!("{:040x}", SEED as u128 * 1_000 + report as u128),"order":report + 1,
            "branch":"master","default_branch":true,"dirty":false,"full":true});
        store::upload_report(
            pool,
            ws,
            "owner",
            serde_json::from_value(json!({"stamp":stamp,"junit":junit(report)}))?,
        )
        .await?;
    }
    Ok(())
}

pub async fn count(pool: &deadpool_postgres::Pool, sql: &str, ws: &str) -> Result<i64> {
    Ok(pool.get().await?.query_one(sql, &[&ws]).await?.get(0))
}
