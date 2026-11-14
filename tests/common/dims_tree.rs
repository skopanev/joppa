//! A large workspace with the filter dimension env: the tree, its records and the stored
//! dimension values of each object.
#![allow(dead_code)]
use anyhow::Result;
use joppa::store;
use serde_json::{Value, json};

pub type Pool = deadpool_postgres::Pool;

pub async fn batch(pool: &Pool, ws: &str, commands: Vec<Value>) -> Result<()> {
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

pub fn node(kind: &str, id: &str, parent: Option<String>) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
        "data":{"title":format!("{kind} {id}"),"owner":"owner","body":"The rule"},"reason":"Create"})
}

/// A workspace of `domains` x 5 capabilities x 6 requirements x `acs` criteria, each with one
/// test node, and the filter dimension env with three values.
pub async fn tree(pool: &Pool, domains: usize, acs: usize) -> Result<(String, Vec<String>)> {
    let ws = format!("write-dims-{}", uuid::Uuid::new_v4().simple());
    let op = json!({"op":"workspace","title":"Write dims"});
    store::execute_input(pool, &ws, "owner", "workspace", &op).await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    batch(pool, &ws, vec![json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "domain":{"title":"Domains","role":"group","status":"children","root":true,"parents":[]},
        "capability":{"title":"Capabilities","role":"group","status":"children","parents":["domain"]},
        "requirement":{"title":"Requirements","role":"requirement","status":"children","parents":["capability"]},
        "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400}}})]).await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    let mut rules = tree["schema"]["test_results"].clone();
    rules["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"env","type":"enum","values":["dev","prod","stage"]}));
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"filters":["env"],
        "test_results":rules,"reason":"Colour the tests by environment"});
    batch(pool, &ws, vec![schema]).await?;
    let (mut commands, mut tests) = (vec![], vec![]);
    for d in 0..domains {
        commands.push(node("domain", &format!("d{d}"), None));
        for c in 0..5 {
            let cap = format!("d{d}c{c}");
            commands.push(node("capability", &cap, Some(format!("domain:d{d}"))));
            for r in 0..6 {
                let req = format!("{cap}r{r}");
                let parent = Some(format!("capability:{cap}"));
                commands.push(node("requirement", &req, parent));
                for a in 0..acs {
                    let ac = format!("{req}a{a}");
                    let mut ac_node = node("ac", &ac, Some(format!("requirement:{req}")));
                    if a % 3 == 0 {
                        ac_node["data"]["env"] = json!("prod");
                    }
                    commands.push(ac_node);
                    let mut test = node("test", &format!("{ac}t"), Some(format!("ac:{ac}")));
                    let name = format!("tests/big.rs > {ac}");
                    test["data"]["title"] = json!(name);
                    tests.push(name);
                    commands.push(test);
                }
            }
        }
    }
    batch(pool, &ws, commands).await?;
    Ok((ws, tests))
}

/// Record `records` runs of every test, each result with an env value.
pub async fn record(pool: &Pool, ws: &str, tests: &[String], records: usize) -> Result<()> {
    let envs = ["dev", "prod", "stage"];
    for k in 0..records {
        let results: Vec<Value> = tests
            .iter()
            .enumerate()
            .map(|(i, t)| json!({"test":t,"status":if (i + k) % 7 == 0 {"fail"} else {"pass"},"env":envs[(i + k) % 3]}))
            .collect();
        for (n, part) in results.chunks(1000).enumerate() {
            let commit = format!("{:07x}", 0xa000000 + k * 16 + n);
            let args = json!({"commit":commit,"results":part});
            store::record_results(pool, ws, "owner", &args).await?;
        }
    }
    Ok(())
}

/// The stored dimension values and tones of each object of `ws`, in address order.
pub async fn stored(
    pool: &Pool,
    ws: &str,
) -> Result<Vec<(String, Option<String>, Option<String>)>> {
    let sql = "SELECT o.address,o.dims::text,o.dim_tones::text FROM joppa.objects o WHERE o.workspace=COALESCE((SELECT workspace FROM joppa.workspace_aliases WHERE name=$1),$1) ORDER BY o.address";
    let rows = pool.get().await?.query(sql, &[&ws]).await?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}
