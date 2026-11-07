//! A typed enum field takes its value from the children, the links or the parent of a node
//! over a real server: each write recomputes the reached values, a read and the data filter
//! give them, and a save refuses a sent value of a computed field.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use record_world::World;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const VALUES: [&str; 4] = ["in_progress", "review", "todo", "done"];

/// The rule of the field stage with the source `from`.
fn stage(from: &str) -> Value {
    json!({"stage":{"type":"enum","values":VALUES,"from":from}})
}

/// Put the types of `put` with set_tree_schema, as the owner. `fields` gives the stage source
/// of the stored types ac and requirement.
async fn schema(w: &World, put: Value, fields: Option<&str>) -> Result<Result<Value, String>> {
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    let mut put = put;
    for kind in ["ac", "requirement"]
        .into_iter()
        .filter(|_| fields.is_some())
    {
        let mut t = tree["schema"]["types"][kind].clone();
        t["fields"] = stage(fields.unwrap_or("own"));
        put[kind] = t;
    }
    let command = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "put_types":put,"filters":["stage"],"reason":"Compute the stage"});
    w.command(&w.owner, command).await
}

/// A node type under `parents` with the stage of `from`.
fn node(title: &str, parents: &[&str], from: &str) -> Value {
    json!({"title":title,"role":"document","status":"none","root":parents.is_empty(),
        "parents":parents,"fields":stage(from)})
}

/// Save a new node, or with `revision` a change of `data`.
async fn save(
    w: &World,
    address: &str,
    revision: u64,
    extra: Value,
) -> Result<Result<Value, String>> {
    let (kind, id) = address.split_once(':').unwrap_or_default();
    let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "data":{"title":id,"owner":"owner"},"reason":"Stage"});
    for (k, v) in extra.as_object().into_iter().flatten() {
        match k.as_str() {
            "parent" | "links" => command[k] = v.clone(),
            _ => command["data"][k] = v.clone(),
        }
    }
    w.command(&w.bot, command).await
}

/// The stage that a read of the node gives, or null.
async fn stage_of(w: &World, address: &str) -> Result<Value> {
    Ok(w.object(address).await?["data"]["stage"].clone())
}

/// Check the stage of each listed node and the nodes of the data filter `stage`.
async fn expect(w: &World, wanted: &[(&str, Value)], filtered: (&str, &[&str])) -> Result<()> {
    for (address, value) in wanted {
        let found = stage_of(w, address).await?;
        ensure!(&found == value, "{address}: {found} and not {value}");
    }
    let args = json!({"parent":"*","data":{"stage":filtered.0},"limit":25});
    let page = w.mcp(&w.bot, "joppa_objects", args).await?;
    let page = page.map_err(anyhow::Error::msg)?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let found: BTreeSet<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
    let wanted: BTreeSet<&str> = filtered.1.iter().copied().collect();
    ensure!(found == wanted, "{}: {found:?}", filtered.0);
    Ok(())
}

// ac:4l6exsb9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_computed_stage_follows_the_children_the_links_and_the_parent() -> Result<()> {
    let w = record_world::world(&[]).await?;
    for (put, error) in [
        (
            json!({"bug":node("Bugs", &[], "sideways")}),
            "stage: from is own, children, links or parent",
        ),
        (
            json!({"bug":{"title":"Bugs","role":"document","status":"none","root":true,"parents":[],
            "fields":{"stage":{"type":"text","from":"links"}}}}),
            "stage: only an enum field takes its value from children or links",
        ),
    ] {
        let reply = schema(&w, put, None).await?;
        ensure!(
            reply.as_ref().is_err_and(|e| e.contains(error)),
            "{reply:?}"
        );
    }
    let types = json!({"task":node("Tasks", &["ac"], "own"),
        "step":node("Steps", &["task"], "parent"),"bug":node("Bugs", &[], "own")});
    schema(&w, types, Some("children"))
        .await?
        .map_err(anyhow::Error::msg)?;
    // b2 stores review while bug keeps its own stage.
    save(&w, "bug:b2", 0, json!({"stage":"review"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    for (address, extra) in [
        ("task:t1", json!({"parent":"ac:a","stage":"todo"})),
        ("task:t2", json!({"parent":"ac:a","stage":"done"})),
        ("step:s", json!({"parent":"task:t1"})),
        (
            "bug:b",
            json!({"links":[{"target":"task:t2","relation":"related"}]}),
        ),
    ] {
        save(&w, address, 0, extra)
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    expect(
        &w,
        &[
            ("ac:a", json!("todo")),
            ("requirement:r", json!("todo")),
            ("step:s", json!("todo")),
            ("bug:b", Value::Null),
        ],
        ("done", &["task:t2"]),
    )
    .await?;
    // The rule change keeps the stored review of b2, and a read no longer gives it.
    schema(&w, json!({"bug":node("Bugs", &[], "links")}), None)
        .await?
        .map_err(anyhow::Error::msg)?;
    expect(
        &w,
        &[("bug:b", json!("done")), ("bug:b2", Value::Null)],
        ("review", &[]),
    )
    .await?;
    // A link from t1 to b counts too: todo comes before done.
    let link = json!({"links":[{"target":"bug:b","relation":"related"}]});
    save(&w, "task:t1", 1, link)
        .await?
        .map_err(anyhow::Error::msg)?;
    expect(
        &w,
        &[("bug:b", json!("todo"))],
        (
            "todo",
            &["task:t1", "step:s", "bug:b", "ac:a", "requirement:r"],
        ),
    )
    .await?;
    // A change of t1 reaches its parents, the step below it and the bug it links to.
    save(&w, "task:t1", 2, json!({"stage":"in_progress"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let all = ["task:t1", "step:s", "bug:b", "ac:a", "requirement:r"];
    expect(&w, &[("task:t2", json!("done"))], ("in_progress", &all)).await?;
    for (address, revision, error) in [
        (
            "bug:b",
            1,
            "stage is computed from links and cannot be set on bug",
        ),
        (
            "ac:a",
            1,
            "stage is computed from children and cannot be set on ac",
        ),
        (
            "step:s",
            1,
            "stage is computed from parent and cannot be set on step",
        ),
    ] {
        let reply = save(&w, address, revision, json!({"stage":"done"})).await?;
        ensure!(
            reply.as_ref().is_err_and(|e| e.contains(error)),
            "{address}: {reply:?}"
        );
    }
    // A save without the field works, also on b2 with its old stored value.
    save(&w, "bug:b2", 1, json!({"title":"B2"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    // An unlink reaches the bug: only t2 links to it now.
    save(&w, "task:t1", 3, json!({"links":[]}))
        .await?
        .map_err(anyhow::Error::msg)?;
    expect(
        &w,
        &[("bug:b", json!("done")), ("ac:a", json!("in_progress"))],
        ("done", &["task:t2", "bug:b"]),
    )
    .await?;
    // A full rebuild of the projection gives the same values.
    schema(&w, json!({"step":node("Step", &["task"], "parent")}), None)
        .await?
        .map_err(anyhow::Error::msg)?;
    expect(
        &w,
        &[
            ("bug:b", json!("done")),
            ("bug:b2", Value::Null),
            ("step:s", json!("in_progress")),
        ],
        (
            "in_progress",
            &["task:t1", "step:s", "ac:a", "requirement:r"],
        ),
    )
    .await?;
    Ok(())
}
