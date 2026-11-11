//! requirement:q438uw0s over a real server: a live save refuses relations and links inside
//! data, unless the type declares the field. An older stored data.links stays readable.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Result, ensure};
use relations_world::{msg, node, schema, stored};
use save_world::{append_event, documents, head, ok, refused, replay, save};
use serde_json::{Value, json};

/// A save_object of `address` at `revision` with `data`. A create stands under requirement:r
/// for a criterion and at the root for another kind.
fn save_of(address: &str, revision: u64, data: Value) -> Value {
    let (kind, id) = address.split_once(':').unwrap_or_default();
    let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "data":data,"reason":"Data relations test"});
    if revision == 0 {
        command["parent"] = if kind == "ac" {
            json!("requirement:r")
        } else {
            Value::Null
        };
    }
    command
}

// deploy-24 AC-3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_live_save_refuses_relations_and_links_inside_data() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    let fields = json!({"fields":{"relations":{"type":"text"}}});
    let risk = node("Risks", "document", &[], fields);
    schema(&w, json!({"put_types":{"risk":risk}}))
        .await?
        .map_err(msg)?;
    // ac:old got data.links from a journal event before this rule.
    w.stop();
    let old = json!({"title":"Old","owner":"owner","links":["x"]});
    append_event(&w, "owner", &save_of("ac:old", 0, old)).await?;
    w.start().await?;
    let read = w.object("ac:old").await?;
    ensure!(read["data"]["links"] == json!(["x"]), "the fixture: {read}");
    // ac has no typed fields: data.relations and data.links are refused.
    ok(&w, save_of("ac:b", 0, json!({"title":"B","owner":"owner"}))).await?;
    let start = head(&w).await?;
    let refusals = [
        (
            json!({"relations":{"related":{"edges":[]}}}),
            "data.relations is not a data field of ac; send relations at the top level of save_object",
        ),
        (
            json!({"links":[]}),
            "data.links is not a data field of ac; send relations at the top level of save_object",
        ),
    ];
    for (data, error) in refusals {
        refused(save(&w, save_of("ac:b", 1, data)).await?, error)?;
    }
    ensure!(head(&w).await? == start, "a refused save appends no event");
    let (revision, _, document) = stored(&w, "ac:b").await?;
    let data = &document["data"];
    ensure!(
        revision == 1 && data.get("relations").is_none() && data.get("links").is_none(),
        "ac:b stores nothing: {document}"
    );
    // risk declares the text field relations.
    let k = json!({"title":"K","owner":"owner","body":"A risk","relations":"high"});
    ok(&w, save_of("risk:k", 0, k)).await?;
    let read = w.object("risk:k").await?;
    ensure!(read["data"]["relations"] == "high", "{read}");
    // A save of ac:old keeps its older data.links, and a null removes it.
    ok(&w, save_of("ac:old", 1, json!({"title":"Old renamed"}))).await?;
    let read = w.object("ac:old").await?;
    ensure!(
        read["data"]["title"] == "Old renamed" && read["data"]["links"] == json!(["x"]),
        "{read}"
    );
    ok(&w, save_of("ac:old", 2, json!({"links":null}))).await?;
    let read = w.object("ac:old").await?;
    ensure!(
        read["revision"] == 3 && read["data"].get("links").is_none(),
        "{read}"
    );
    // The replay of the journal gives the same documents.
    let before = documents(&w).await?;
    replay(&w).await?;
    let after = documents(&w).await?;
    ensure!(
        before == after,
        "the replay changed the documents:\n{before:?}\n{after:?}"
    );
    Ok(())
}
