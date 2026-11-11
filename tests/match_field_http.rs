//! requirement:result-shape-in-schema and requirement:test-nodes over a real server:
//! test_results.match_field names a typed field of each test type, and a recorded name binds
//! by that field after the address and the title.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{msg, node, schema, tone, tree};
use save_world::{ok, refused};
use serde_json::{Value, json};

const RULE: &str = "test_results.match_field names a field of the test type";

/// A type `title` with role test under ac, with the typed text field case when `case`.
fn test_type(title: &str, case: bool) -> Value {
    let mut extra = json!({"status":"own","title_preview_length":400});
    if case {
        extra["fields"] = json!({"case":{"type":"text"}});
    }
    node(title, "test", &["ac"], extra)
}

/// The current test_results block, with match_field `field` or without match_field.
async fn block(w: &World, field: Option<Value>) -> Result<Value> {
    let mut block = tree(w, &w.owner, None).await?["schema"]["test_results"].clone();
    block.as_object_mut().map(|m| m.remove("match_field"));
    if let Some(field) = field {
        block["match_field"] = field;
    }
    Ok(json!({"test_results":block}))
}

/// The revision and the stored match_field of the tree schema.
async fn schema_state(w: &World) -> Result<(Value, Value)> {
    let schema = tree(w, &w.owner, None).await?["schema"].clone();
    Ok((
        schema["revision"].clone(),
        schema["test_results"]["match_field"].clone(),
    ))
}

/// The workspace with the typed field case on type test and match_field "case".
async fn with_case(w: &World) -> Result<()> {
    let types = json!({"put_types":{"test":test_type("Tests", true)}});
    schema(w, types).await?.map_err(msg)?;
    schema(w, block(w, Some(json!("case"))).await?)
        .await?
        .map_err(msg)?;
    Ok(())
}

// deploy-24 AC-6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn set_tree_schema_checks_test_results_match_field() -> Result<()> {
    let w = record_world::world(&[]).await?;
    with_case(&w).await?;
    ensure!(schema_state(&w).await?.1 == "case", "joppa_tree gives case");
    let integer = "invalid test_results: invalid type: integer `7`, expected a string";
    let refusals = [
        (block(&w, Some(json!("nope"))).await?, RULE),
        (block(&w, Some(json!(7))).await?, integer),
        (
            json!({"put_types":{"test":test_type("Tests", false)}}),
            RULE,
        ),
        (json!({"remove_types":["test"]}), RULE),
        // A second type of role test needs the field too.
        (
            json!({"put_types":{"check":test_type("Checks", false)}}),
            RULE,
        ),
    ];
    for (change, error) in refusals {
        let before = schema_state(&w).await?;
        refused(schema(&w, change.clone()).await?, error)?;
        ensure!(
            schema_state(&w).await? == before,
            "{change}: the refusal keeps the schema"
        );
    }
    let check = json!({"put_types":{"check":test_type("Checks", true)}});
    schema(&w, check).await?.map_err(msg)?;
    ensure!(schema_state(&w).await?.1 == "case", "two test types");
    // A block without match_field is stored, and joppa_tree gives no match_field.
    schema(&w, block(&w, None).await?).await?.map_err(msg)?;
    let read = tree(&w, &w.owner, None).await?;
    let rules = &read["schema"]["test_results"];
    ensure!(
        rules.is_object() && rules.get("match_field").is_none_or(Value::is_null),
        "{read}"
    );
    Ok(())
}

/// The stored result statuses of the test node `address`, oldest first.
async fn results(w: &World, address: &str) -> Result<Vec<String>> {
    let sql = "SELECT t.status::text FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND t.address=$2 ORDER BY r.id";
    let rows = w.pool.get().await?.query(sql, &[&w.ws, &address]).await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

// deploy-24 AC-7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_result_binds_by_match_field_after_the_address_and_the_title() -> Result<()> {
    let w = record_world::world(&[]).await?;
    with_case(&w).await?;
    // No title of T1 to T4 equals a recorded name. T5 has no case.
    for (id, title, case) in [
        ("t1", "Refund test", Some("refund > over")),
        ("t2", "Duplicate one", Some("b > dup")),
        ("t3", "Duplicate two", Some("b > dup")),
        ("t4", "Logout case", Some("a > logout")),
        ("t5", "a > logout", None),
    ] {
        let mut data = json!({"title":title,"owner":"owner"});
        if let Some(case) = case {
            data["case"] = json!(case);
        }
        ok(
            &w,
            json!({"op":"save_object","kind":"test","id":id,"revision":0,"parent":"ac:a",
                "data":data,"reason":"Bind"}),
        )
        .await?;
    }
    let pass = |test: &str| json!({"test":test,"status":"pass"});
    let run = json!([
        pass("refund > over"),
        pass(" refund > over"),
        pass("a > logout"),
        pass("b > dup")
    ]);
    let first = w.recorded(&"a".repeat(40), run).await?;
    ensure!(
        first["unbound"] == json!([" refund > over", "b > dup"]),
        "{first}"
    );
    for (address, green) in [
        ("test:t1", true),
        ("test:t2", false),
        ("test:t3", false),
        ("test:t4", false),
        ("test:t5", true),
    ] {
        let tone = tone(&w, address).await?;
        ensure!((tone == "green") == green, "{address}: {tone}");
    }
    for (address, rows) in [
        ("test:t1", vec!["pass"]),
        ("test:t2", vec![]),
        ("test:t3", vec![]),
        ("test:t4", vec![]),
        ("test:t5", vec!["pass"]),
    ] {
        let stored = results(&w, address).await?;
        ensure!(stored == rows, "{address}: {stored:?}");
    }
    // Without match_field the name binds no node, and T1 keeps its result.
    schema(&w, block(&w, None).await?).await?.map_err(msg)?;
    let fail = json!([{"test":"refund > over","status":"fail"}]);
    let second = w.recorded(&"b".repeat(40), fail).await?;
    ensure!(second["unbound"] == json!(["refund > over"]), "{second}");
    ensure!(tone(&w, "test:t1").await? == "green", "T1 stays green");
    ensure!(
        results(&w, "test:t1").await? == ["pass"],
        "the fail is not stored under T1"
    );
    Ok(())
}
