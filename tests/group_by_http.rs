//! group_by over a real server gives the count of each group in one read. domain:d holds 4
//! criteria (2 green, 1 red, 1 blue), 6 test nodes and 3 tasks. The criterion type takes the
//! field stage from its children, and alex holds 2 tasks.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/computed_world.rs"]
mod computed_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use computed_world::{node, save, schema};
use record_world::World;
use serde_json::{Value, json};
use std::cmp::Ordering;

/// The field stage with the source `from`.
fn stage(from: &str) -> Value {
    json!({"fields":{"stage":{"type":"enum","values":["todo","done"],"from":from}}})
}

/// The types, domain:d and its criteria, test nodes and tasks with their results.
async fn fixture(w: &World) -> Result<()> {
    let mut ac = node(
        "Criteria",
        "ac",
        &["requirement", "domain"],
        stage("children"),
    );
    ac["status"] = json!("children");
    let mut task = node("Tasks", "task", &["ac"], stage("own"));
    task["status"] = json!("none");
    let types = json!({"domain":node("Domains", "group", &[], json!({})),"ac":ac,"task":task});
    schema(w, types).await?.map_err(anyhow::Error::msg)?;
    let mut nodes = vec![("domain:d".to_owned(), json!({}))];
    for ac in ["ac:g1", "ac:g2", "ac:r1", "ac:b1"] {
        nodes.push((ac.to_owned(), json!({"parent":"domain:d"})));
    }
    for (n, ac) in ["g1", "g1", "g2", "g2", "r1", "b1"].iter().enumerate() {
        let title = format!("tests/group_by.rs > {ac} {n}");
        let extra = json!({"parent":format!("ac:{ac}"),"title":title});
        nodes.push((format!("test:x{n}"), extra));
    }
    for (id, ac, stage, alex) in [
        ("t1", "g1", "done", true),
        ("t2", "g2", "done", true),
        ("t3", "r1", "todo", false),
    ] {
        let mut extra = json!({"parent":format!("ac:{ac}"),"stage":stage});
        if alex {
            extra["assignee"] = json!("alex");
        }
        nodes.push((format!("task:{id}"), extra));
    }
    for (address, extra) in nodes {
        let saved = save(w, &address, 0, extra).await?;
        saved.map_err(|e| anyhow::anyhow!("{address}: {e}"))?;
    }
    let result = |n: usize, status: &str| {
        let ac = ["g1", "g1", "g2", "g2", "r1"][n];
        json!({"test":format!("tests/group_by.rs > {ac} {n}"),"status":status})
    };
    let results = [
        (0, "pass"),
        (1, "pass"),
        (2, "pass"),
        (3, "pass"),
        (4, "fail"),
    ];
    let results: Vec<Value> = results.iter().map(|(n, s)| result(*n, s)).collect();
    w.recorded(&"9".repeat(40), json!(results)).await?;
    Ok(())
}

/// Ascending order of one key value: null comes last.
fn order(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Greater,
        (_, Value::Null) => Ordering::Less,
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap_or(0.0), b.as_f64().unwrap_or(0.0));
            a.partial_cmp(&b).unwrap_or(Ordering::Equal)
        }
        _ => a.to_string().cmp(&b.to_string()),
    }
}

/// Check the shape {"total":N,"groups":[{<key>:<value>,...,"count":C}]} of a reply for `keys`.
/// The counts add up to total, and the groups come in ascending key order with null last.
/// Gives the key values and the count of each group.
fn groups(what: &str, reply: &Value, keys: &[&str]) -> Result<Vec<(Vec<Value>, u64)>> {
    let map = reply.as_object().context("an object")?;
    let top = map.keys().all(|k| k == "total" || k == "groups") && map.len() == 2;
    ensure!(top, "{what}: only total and groups: {reply}");
    let total = reply["total"].as_u64().context("total")?;
    let list = reply["groups"].as_array().context("groups")?;
    let mut out: Vec<(Vec<Value>, u64)> = Vec::new();
    for group in list {
        let fields = group.as_object().context("a group object")?;
        let right = fields.len() == keys.len() + 1 && keys.iter().all(|k| fields.contains_key(*k));
        ensure!(
            right,
            "{what}: a group has the keys {keys:?} and count: {group}"
        );
        let count = group["count"].as_u64().filter(|c| *c > 0);
        let count = count.with_context(|| format!("{what}: count {group}"))?;
        out.push((keys.iter().map(|k| group[*k].clone()).collect(), count));
    }
    let sum: u64 = out.iter().map(|(_, c)| c).sum();
    ensure!(
        sum == total,
        "{what}: the counts add up to {sum}, total {total}: {reply}"
    );
    for pair in out.windows(2) {
        let (a, b) = (&pair[0].0, &pair[1].0);
        let step = a.iter().zip(b).map(|(x, y)| order(x, y));
        let rank = step.fold(Ordering::Equal, Ordering::then);
        ensure!(
            rank == Ordering::Less,
            "{what}: ascending key order: {reply}"
        );
    }
    Ok(out)
}

/// joppa_objects below domain:d with `group_by` and the keys of `extra`.
async fn grouped(w: &World, group_by: Value, extra: Value) -> Result<Result<Value, String>> {
    let mut args = json!({"parent":"domain:d","depth":100,"group_by":group_by});
    for (k, v) in extra.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    w.mcp(&w.bot, "joppa_objects", args).await
}

/// The checked groups of a grouped read that must succeed.
async fn read(w: &World, keys: &[&str]) -> Result<Vec<(Vec<Value>, u64)>> {
    let reply = grouped(w, json!(keys), json!({})).await?;
    let reply = reply.map_err(anyhow::Error::msg)?;
    ensure!(reply["total"] == 13, "group_by {keys:?}: 13 nodes: {reply}");
    groups(&format!("group_by {keys:?}"), &reply, keys)
}

/// One group with the key values `values` and the count `count`.
fn one(values: &[Value], count: u64) -> (Vec<Value>, u64) {
    (values.to_vec(), count)
}

// ac:bcpvdncp
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn group_by_gives_the_count_of_each_group_in_one_read() -> Result<()> {
    let w = record_world::world(&[]).await?;
    fixture(&w).await?;
    let (ac, test) = (json!("ac"), json!("test"));
    let kinds = read(&w, &["kind"]).await?;
    let wanted = [
        one(&[json!("ac")], 4),
        one(&[json!("task")], 3),
        one(&[json!("test")], 6),
    ];
    ensure!(kinds == wanted, "group_by kind: {kinds:?}");
    let pairs = read(&w, &["kind", "status"]).await?;
    let of = |kind: &Value| -> Vec<(Vec<Value>, u64)> {
        let mine = pairs.iter().filter(|(v, _)| &v[0] == kind);
        mine.cloned().collect()
    };
    let tones = |kind: &Value, blue: u64, green: u64, red: u64| {
        let t = |tone: &str, n: u64| one(&[kind.clone(), json!(tone)], n);
        vec![t("blue", blue), t("green", green), t("red", red)]
    };
    ensure!(
        of(&ac) == tones(&ac, 1, 2, 1),
        "the criteria by status: {pairs:?}"
    );
    ensure!(
        of(&test) == tones(&test, 1, 4, 1),
        "the tests by status: {pairs:?}"
    );
    let stages = read(&w, &["stage"]).await?;
    let wanted = [
        one(&[json!("done")], 4),
        one(&[json!("todo")], 2),
        one(&[Value::Null], 7),
    ];
    ensure!(stages == wanted, "group_by stage: {stages:?}");
    let data = read(&w, &["data.stage"]).await?;
    ensure!(data == stages, "data.stage {data:?} and stage {stages:?}");
    let holders = read(&w, &["assignee"]).await?;
    let wanted = [one(&[json!("alex")], 2), one(&[Value::Null], 11)];
    ensure!(holders == wanted, "group_by assignee: {holders:?}");
    let path = "/objects?parent=domain:d&depth=100&group_by=kind,status";
    let http = blue_world::get(&w, &w.bot, path).await?;
    let http = groups(&format!("GET {path}"), &http, &["kind", "status"])?;
    ensure!(http == pairs, "HTTP {http:?} and MCP {pairs:?}");
    for (group_by, extra, cause) in [
        (json!([]), json!({}), "group_by"),
        (
            json!(["kind", "status", "stage", "assignee"]),
            json!({}),
            "group_by",
        ),
        (json!(["kind"]), json!({"fields":["title"]}), "fields"),
    ] {
        let reply = grouped(&w, group_by.clone(), extra.clone()).await?;
        let named = reply.as_ref().is_err_and(|e| e.contains(cause));
        ensure!(
            named,
            "{group_by} {extra}: a refusal names {cause}: {reply:?}"
        );
    }
    Ok(())
}
