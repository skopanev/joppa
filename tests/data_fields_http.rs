//! ac:zytodppm over a real server: fields selects the keys of each item of POST /api/data, and
//! a reply over max_chars gives 422 with the character count of the full reply.
#[path = "common/data_tree.rs"]
mod data_tree;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use data_tree::{node, put_types, save};
use read_key::{data, read, with_key};
use record_world::World;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// box:root > type_a:a1. type_a declares the text field area. a1 has a title, a body, an owner
/// and an area, and no assignee.
async fn tree(w: &World) -> Result<()> {
    let fields = json!({"area":{"type":"text"}});
    put_types(
        w,
        json!({
            "box": node("Boxes", "children", &[], json!({})),
            "type_a": node("Type A", "own", &["box"], fields),
        }),
    )
    .await?;
    save(w, "box:root", None, json!({})).await?;
    let a1 = json!({"title":"First item","body":"The body of a1","owner":"owner","area":"north"});
    save(w, "type_a:a1", Some("box:root"), a1).await
}

/// The keys of an object.
fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k.clone())
        .collect()
}

/// The one item of a reply.
fn item(reply: &Value) -> Result<&Value> {
    let items = reply["items"].as_array();
    ensure!(items.is_some_and(|i| i.len() == 1), "one item: {reply}");
    Ok(&reply["items"][0])
}

/// Check that `value` has exactly the keys `wanted`.
fn exactly(value: &Value, wanted: &[&str]) -> Result<()> {
    let wanted: BTreeSet<String> = wanted.iter().map(|k| (*k).to_owned()).collect();
    ensure!(keys(value) == wanted, "{value}: keys should be {wanted:?}");
    Ok(())
}

// ac:zytodppm
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_reply_names_the_workspace_and_the_journal_position_and_items() -> Result<()> {
    with_key(tree, async |w, key| {
        let reply = read(w, key, json!({"root":"type_a:a1"})).await?;
        exactly(&reply, &["workspace", "position", "items"])?;
        ensure!(reply["workspace"] == json!(w.ws), "{reply}");
        // The journal position is the position of the newest event of the workspace.
        let sql = "SELECT max(position) FROM joppa.events WHERE workspace=$1";
        let newest: i64 = w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0);
        let position = json!(newest);
        ensure!(
            reply["position"] == position,
            "{reply}: position {position}"
        );
        Ok(())
    })
    .await
}

// ac:zytodppm
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn without_fields_an_item_has_address_kind_revision_title_and_body() -> Result<()> {
    with_key(tree, async |w, key| {
        let reply = read(w, key, json!({"root":"type_a:a1"})).await?;
        let a1 = item(&reply)?;
        exactly(a1, &["address", "kind", "revision", "title", "body"])?;
        let revision = w.object("type_a:a1").await?["revision"].clone();
        let wanted = json!({"address":"type_a:a1","kind":"type_a","revision":revision,
            "title":"First item","body":"The body of a1"});
        ensure!(*a1 == wanted, "{a1}");
        Ok(())
    })
    .await
}

// ac:zytodppm
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn fields_select_declared_and_named_keys_and_leave_out_a_null_value() -> Result<()> {
    with_key(tree, async |w, key| {
        let fields = json!(["area", "status", "parent", "owner", "assignee"]);
        let reply = read(w, key, json!({"root":"type_a:a1","fields":fields})).await?;
        let a1 = item(&reply)?;
        let wanted = [
            "address", "kind", "revision", "area", "status", "parent", "owner",
        ];
        exactly(a1, &wanted)?;
        ensure!(a1["area"] == "north" && a1["owner"] == "owner", "{a1}");
        ensure!(a1["parent"] == "box:root", "{a1}");
        ensure!(a1["status"].as_str().is_some_and(|s| !s.is_empty()), "{a1}");
        Ok(())
    })
    .await
}

// ac:zytodppm
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_unknown_or_empty_fields_an_extra_key_or_a_max_chars_out_of_range_gives_400()
-> Result<()> {
    with_key(tree, async |w, key| {
        for extra in [
            json!({"fields":["work_ready"]}),
            json!({"fields":["nope"]}),
            json!({"fields":[]}),
            json!({"extra":true}),
            json!({"max_chars":0}),
            json!({"max_chars":262_145}),
        ] {
            let mut body = json!({"root":"type_a:a1"});
            for (k, v) in extra.as_object().into_iter().flatten() {
                body[k] = v.clone();
            }
            let (code, reply) = data(w, Some(key), &body).await?;
            ensure!(code == 400, "{body}: {code} {reply}");
        }
        Ok(())
    })
    .await
}

// ac:zytodppm
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_reply_over_max_chars_gives_422_with_the_full_character_count() -> Result<()> {
    with_key(tree, async |w, key| {
        let full = read(w, key, json!({"root":"type_a:a1"})).await?;
        let count = serde_json::to_string(&full)?.chars().count();
        ensure!(count > 50, "the full reply has {count} characters");
        let body = json!({"root":"type_a:a1","max_chars":50});
        let (code, reply) = data(w, Some(key), &body).await?;
        let error = format!("response is {count} characters, max_chars is 50");
        ensure!(
            (code, &reply) == (422, &json!({"error": error})),
            "{code} {reply}, not 422 {error:?}"
        );
        Ok(())
    })
    .await
}
