//! requirement:vqlbur46 over a real server: each read gives the tone blue where the older
//! release gave grey. W holds requirement R, criterion A under R and test node T under A
//! without a result. Workspace V is bound to node doc:v of W. The agent bot is a member of V,
//! and member alex is not.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Context, Result, ensure};
use blue_world::{T, get, no_grey, test_node, workspace};
use record_world::World;
use serde_json::{Value, json};

/// The tone and the label of a detail read: MCP gives status, HTTP gives signal.
fn tone_label(read: &Value) -> (Value, Value) {
    let s = if read["status"].is_object() {
        &read["status"]
    } else {
        &read["signal"]
    };
    (s["tone"].clone(), s["label"].clone())
}

/// The detail reads of `address` through MCP and HTTP give blue with `label`.
async fn detail(w: &World, address: &str, label: &str) -> Result<()> {
    let mcp = w.object(address).await?;
    let http = get(w, &w.bot, &format!("/objects?address={address}")).await?;
    for (way, read) in [("joppa_objects", mcp), ("GET /objects", http)] {
        no_grey(&format!("{way} {address}"), &read)?;
        let found = tone_label(&read);
        ensure!(
            found == (json!("blue"), json!(label)),
            "{way} {address}: {found:?} {read}"
        );
    }
    Ok(())
}

/// The status of `address` in a list page: a tone, or an object with the tone.
fn listed_tone(page: &Value, address: &str) -> Value {
    let items = page["items"].as_array().into_iter().flatten();
    let item = items.into_iter().find(|i| i["address"] == address);
    let item = item.cloned().unwrap_or_default();
    for key in ["status", "signal"] {
        if item[key].is_string() {
            return item[key].clone();
        }
        if item[key]["tone"].is_string() {
            return item[key]["tone"].clone();
        }
    }
    Value::Null
}

// ac:idxkripi
// test:bsmog3s4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn every_read_gives_blue_where_it_gave_grey() -> Result<()> {
    let w = relations_world::world(&[T]).await?;
    let t = test_node(&w).await?;
    let text = json!({"body":"Blue fixture criterion"});
    relations_world::change(&w, "ac:a", text).await?;
    let v = workspace(&w, "Bound").await?;
    let bind = json!({"workspace":v});
    relations_world::save_as(&w, &w.owner, "doc:v", 0, bind)
        .await?
        .map_err(anyhow::Error::msg)?;

    // T, A and R in detail.
    detail(&w, &t, "Awaits checks").await?;
    detail(&w, "ac:a", "Children await checks").await?;
    detail(&w, "requirement:r", "Children await checks").await?;
    // T, A and R in a list of joppa_objects and of GET /objects.
    let args = json!({"parent":"*","fields":["title","status"],"limit":25});
    let mcp = w.mcp(&w.bot, "joppa_objects", args).await?;
    let mcp = mcp.map_err(anyhow::Error::msg)?;
    let http = get(&w, &w.bot, "/objects?parent=*&fields=signal&limit=100").await?;
    for (way, page) in [("joppa_objects", &mcp), ("GET /objects", &http)] {
        no_grey(way, page)?;
        for address in [t.as_str(), "ac:a", "requirement:r"] {
            let tone = listed_tone(page, address);
            ensure!(tone == "blue", "{way} {address}: {tone} {page}");
        }
    }
    // joppa_search finds A and gives no grey.
    let args = json!({"query":"Blue fixture criterion","limit":25});
    let found = w.mcp(&w.bot, "joppa_search", args).await?;
    let found = found.map_err(anyhow::Error::msg)?;
    no_grey("joppa_search", &found)?;
    let items = found["items"].as_array().context("search items")?;
    ensure!(items.iter().any(|i| i["address"] == "ac:a"), "{found}");
    // store::test_status of A.
    let status = w.a_status(&w.bot).await?;
    no_grey("store::test_status", &status)?;
    ensure!(status["status"] == "blue", "{status}");

    // The node bound to V, as bot and as alex read it.
    let bot = w.object("doc:v").await?;
    no_grey("doc:v of bot", &bot)?;
    let child = &bot["child_workspace"];
    ensure!(
        child["workspace"] == json!(v) && child["access"] == true && child["status"] == "blue",
        "bot: {bot}"
    );
    let alex = w
        .mcp(&w.alex, "joppa_objects", json!({"address":"doc:v"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        alex["child_workspace"] == json!({"workspace":v,"access":false,"status":"blue"}),
        "alex: {alex}"
    );
    let http = get(&w, &w.alex, "/objects?address=doc:v").await?;
    no_grey("GET /objects doc:v of alex", &http)?;
    // The board data of bot gives no grey.
    let board = get(&w, &w.bot, "/board").await?;
    no_grey("GET /board", &board)?;
    Ok(())
}

// ac:idxkripi
// test:82wg3kcv
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn store_test_status_gives_blue_for_a_criterion_without_a_result() -> Result<()> {
    let w = record_world::world(&[T]).await?;
    let status = joppa::store::test_status(&w.pool, &w.ws, "bot", "ac:a").await?;
    no_grey("store::test_status", &status)?;
    let test = record_world::test(&status, T).clone();
    ensure!(
        status["status"] == "blue" && test["status"] == "not_run",
        "{status}"
    );
    // A pass makes A green, and a later fail makes it red: blue is only the waiting tone.
    w.recorded(&"a".repeat(40), json!([{"test":T,"status":"pass"}]))
        .await?;
    let status = joppa::store::test_status(&w.pool, &w.ws, "bot", "ac:a").await?;
    ensure!(status["status"] == "green", "{status}");
    Ok(())
}
