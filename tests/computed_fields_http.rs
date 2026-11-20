//! A typed enum field takes its value from the children, the links or the parent of a node
//! over a real server: each write recomputes the reached values, a read and the data filter
//! give them, and a save refuses a sent value of a computed field.
#[path = "common/manual_check.rs"]
mod manual_check;
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
            "parent" | "relations" => command[k] = v.clone(),
            _ => command["data"][k] = v.clone(),
        }
    }
    w.command(&w.bot, command).await
}

/// The stage that a read of the node gives, or null: from `data` when own, else `computed`.
async fn stage_of(w: &World, address: &str) -> Result<Value> {
    let card = w.object(address).await?;
    let kind = address.split_once(':').map_or("", |(k, _)| k);
    let tree = w
        .mcp(&w.bot, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let from = tree["schema"]["types"][kind]["fields"]["stage"]["from"]
        .as_str()
        .unwrap_or("own");
    Ok(if from == "own" {
        card["data"]["stage"].clone()
    } else {
        card["computed"]["stage"].clone()
    })
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
            "stage: from is own, status, children, links or parent",
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
            json!({"relations":{"related":{"edges":[{"to":"task:t2"}]}}}),
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
    // A related edge from t1 to b counts too: todo comes before done.
    let link = json!({"relations":{"related":{"edges":[{"to":"bug:b"}]}}});
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
    // The removal of the related edge reaches the bug: only t2 links to it now.
    save(&w, "task:t1", 3, json!({"relations":{"related":null}}))
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

// ac:2n1hmhfi
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_computed_stage_from_status_follows_tones_without_resetting_tests() -> Result<()> {
    let w = record_world::world(&["t1", "t2"]).await?;
    let stages = ["to_decide", "to_prepare", "in_progress", "done"];
    let map = json!({"green":"done","blue":"in_progress","red":"to_decide","orange":"to_prepare"});

    // Check schema refusals for invalid status maps and map on non-status source.
    for (bad_field, expected) in [
        (
            json!({"type":"enum","values":stages,"from":"status"}),
            "from status takes an enum field with a map of tones",
        ),
        (
            json!({"type":"enum","values":stages,"from":"status","map":{"purple":"done"}}),
            "a map key is a tone: red, orange, yellow, blue or green",
        ),
        (
            json!({"type":"enum","values":stages,"from":"status","map":{"green":"later"}}),
            "the map value later is not one of the values",
        ),
        (
            json!({"type":"enum","values":stages,"from":"children","map":{"green":"done"}}),
            "only from status takes a map",
        ),
    ] {
        let tree = w
            .mcp(&w.owner, "joppa_tree", json!({}))
            .await?
            .map_err(anyhow::Error::msg)?;
        let mut ac = tree["schema"]["types"]["ac"].clone();
        ac["fields"] = json!({"stage": bad_field});
        let reply = w
            .command(
                &w.owner,
                json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"put_types":{"ac":ac},"reason":"Bad status map"}),
            )
            .await?;
        ensure!(
            reply.as_ref().is_err_and(|e| e.contains(expected)),
            "expected {expected}, got {reply:?}"
        );
    }

    // Configure ac from status, requirement from children, and bug from links.
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let mut ac = tree["schema"]["types"]["ac"].clone();
    ac["fields"] = json!({"stage":{"type":"enum","values":stages,"from":"status","map":map}});
    let mut req = tree["schema"]["types"]["requirement"].clone();
    req["fields"] = json!({"stage":{"type":"enum","values":stages,"from":"children"}});
    let bug = json!({
        "title":"Bugs","role":"document","status":"none","root":true,"parents":[],
        "fields":{"stage":{"type":"enum","values":stages,"from":"links"}}
    });
    w.command(
        &w.owner,
        json!({
            "op":"set_tree_schema",
            "revision":tree["schema"]["revision"],
            "put_types":{"ac":ac,"requirement":req,"bug":bug},
            "filters":["stage"],
            "reason":"Compute stage from status"
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Add criterion B without tests (orange -> to_prepare) and bug:b1 linked to ac:a.
    save(&w, "ac:b", 0, json!({"parent":"requirement:r"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    save(
        &w,
        "bug:b1",
        0,
        json!({"relations":{"related":{"edges":[{"to":"ac:a"}]}}}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Initially ac:a is blue (in_progress), ac:b is orange (to_prepare), and R takes to_prepare.
    expect(
        &w,
        &[
            ("ac:a", json!("in_progress")),
            ("bug:b1", json!("in_progress")),
            ("ac:b", json!("to_prepare")),
            ("requirement:r", json!("to_prepare")),
        ],
        ("in_progress", &["ac:a", "bug:b1"]),
    )
    .await?;

    let record = async |commit_n: u32, results: Value| -> Result<()> {
        w.mcp(
            &w.bot,
            "joppa_record_results",
            json!({"commit":format!("{commit_n:040x}"),"results":results}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        Ok(())
    };

    // Pass both tests of ac:a -> ac:a and bug:b1 become done without resetting any test.
    record(
        1,
        json!([{"test":"t1","status":"pass"},{"test":"t2","status":"pass"}]),
    )
    .await?;
    expect(
        &w,
        &[
            ("ac:a", json!("done")),
            ("bug:b1", json!("done")),
            ("ac:b", json!("to_prepare")),
            ("requirement:r", json!("to_prepare")),
        ],
        ("done", &["ac:a", "bug:b1"]),
    )
    .await?;

    // Fail t1 only -> ac:a becomes red (to_decide), R becomes to_decide, and t2 keeps its earlier pass without reset.
    record(2, json!([{"test":"t1","status":"fail"}])).await?;
    expect(
        &w,
        &[
            ("ac:a", json!("to_decide")),
            ("bug:b1", json!("to_decide")),
            ("ac:b", json!("to_prepare")),
            ("requirement:r", json!("to_decide")),
        ],
        ("to_decide", &["ac:a", "bug:b1", "requirement:r"]),
    )
    .await?;
    let a_obj = w.object("ac:a").await?;
    ensure!(
        a_obj["status"]["tone"] == json!("red"),
        "expected red tone on ac:a: {a_obj}"
    );

    // Pass t1 alone on commit 3 -> because t2 was never reset, ac:a turns green (done) immediately.
    record(3, json!([{"test":"t1","status":"pass"}])).await?;
    expect(
        &w,
        &[
            ("ac:a", json!("done")),
            ("bug:b1", json!("done")),
            ("ac:b", json!("to_prepare")),
            ("requirement:r", json!("to_prepare")),
        ],
        ("done", &["ac:a", "bug:b1"]),
    )
    .await?;

    // Manual save of stage on ac:a is refused.
    let cur_rev = w.object("ac:a").await?["revision"].as_u64().unwrap_or(1);
    let refused = save(&w, "ac:a", cur_rev, json!({"stage":"to_decide"})).await?;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains("stage is computed from status and cannot be set on ac")),
        "expected computed refusal, got {refused:?}"
    );

    // Also verify manual check expiry updates status-computed stage without a journal save.
    save(&w, "ac:m", 0, json!({"parent":"requirement:r"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let d = chrono::Utc::now().date_naive() + chrono::TimeDelta::days(2);
    manual_check::check(
        &w,
        "ac:m",
        json!({"check_result":"pass","valid_until":d.format("%Y-%m-%d").to_string()}),
    )
    .await?;
    ensure!(
        stage_of(&w, "ac:m").await? == json!("done"),
        "manual pass must set ac:m stage to done"
    );
    let expired = joppa::store::expire_manual_checks(&w.pool, &w.ws, d).await?;
    ensure!(expired == 1, "expected 1 expired check, got {expired}");
    ensure!(
        stage_of(&w, "ac:m").await? == json!("in_progress"),
        "expired manual check must turn ac:m blue -> in_progress"
    );
    Ok(())
}
