//! requirement:check-list-frozen over a real server: a type of the tree schema takes the flag
//! source: true, joppa_tree gives it back, and a flag that is not a boolean is refused.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// The joppa_tree reply for the owner.
async fn tree(w: &record_world::World) -> Result<Value> {
    w.mcp(&w.owner, "joppa_tree", json!({}))
        .await?
        .map_err(|e| anyhow::anyhow!("joppa_tree: {e}"))
}

/// A root document type with the `source` value, when it is given.
fn document(title: &str, source: Option<Value>) -> Value {
    let mut t = json!({"title":title,"role":"document","status":"none","root":true,"parents":[]});
    if let Some(source) = source {
        t["source"] = source;
    }
    t
}

/// set_tree_schema with one type in put_types, at the current schema revision.
async fn put(
    w: &record_world::World,
    kind: &str,
    t: Value,
    revision: &Value,
) -> Result<Result<Value, String>> {
    let command = json!({"op":"set_tree_schema","revision":revision,"put_types":{kind:t},
        "reason":"Name the source types"});
    w.command(&w.owner, command).await
}

// ac:pkutoxs5
// test:6cz97n66
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_type_flag_source_true_is_saved_and_read_and_a_bad_flag_is_refused() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let n = tree(&w).await?["schema"]["revision"]
        .as_u64()
        .unwrap_or_default();
    let first = put(&w, "src", document("Sources", Some(json!(true))), &json!(n)).await?;
    first.map_err(|e| anyhow::anyhow!("source true: {e}"))?;
    let after_first = tree(&w).await?;
    let types = &after_first["schema"]["types"];
    ensure!(
        after_first["schema"]["revision"] == n + 1,
        "{}",
        after_first["schema"]
    );
    ensure!(types["src"]["source"] == true, "src: {}", types["src"]);
    ensure!(
        types["requirement"].get("source").is_none(),
        "a type without the flag: {}",
        types["requirement"]
    );
    let second = put(
        &w,
        "src2",
        document("Other", Some(json!(false))),
        &json!(n + 1),
    )
    .await?;
    second.map_err(|e| anyhow::anyhow!("source false: {e}"))?;
    let after_second = tree(&w).await?;
    let types = &after_second["schema"]["types"];
    ensure!(
        after_second["schema"]["revision"] == n + 2,
        "{}",
        after_second["schema"]
    );
    ensure!(types["src"]["source"] == true, "src: {}", types["src"]);
    for kind in ["src2", "requirement", "ac"] {
        ensure!(
            types[kind].is_object() && types[kind].get("source").is_none(),
            "{kind} must have no source key: {}",
            types[kind]
        );
    }
    let third = put(
        &w,
        "src3",
        document("Bad", Some(json!("yes"))),
        &json!(n + 2),
    )
    .await?;
    ensure!(third.is_err(), "source \"yes\" was saved: {third:?}");
    let after_third = tree(&w).await?;
    ensure!(
        after_third["schema"]["revision"] == n + 2
            && after_third["schema"]["types"].get("src3").is_none(),
        "the refused command changed the schema: {}",
        after_third["schema"]
    );
    Ok(())
}
