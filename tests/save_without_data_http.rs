//! requirement:edit-makes-revision over a real server: an update by save_object without data
//! keeps the stored data. A clean save and a create without data are refused.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{edges, last_event, relations, stored, to};
use save_world::{exists, head, ok, refused, save};
use serde_json::{Value, json};

/// A save_object of ac:b at `revision` without data, with the command keys of `extra`.
fn update(revision: u64, extra: Value) -> Value {
    let mut command = json!({"op":"save_object","kind":"ac","id":"b","revision":revision,
        "reason":"Save without data"});
    for (k, v) in extra.as_object().into_iter().flatten() {
        command[k] = v.clone();
    }
    command
}

/// ac:b reads at `revision` under `parent` with the data `data`, and the last journal event
/// names the whole data. Gives the read.
async fn kept(w: &World, revision: u64, parent: &str, data: &Value) -> Result<Value> {
    let read = w.object("ac:b").await?;
    ensure!(
        read["revision"] == revision && read["parent"] == parent && &read["data"] == data,
        "revision {revision}: {read}"
    );
    let event = last_event(w).await?;
    ensure!(
        event["id"] == "b" && &event["data"] == data,
        "the journal event names the whole data: {event}"
    );
    Ok(read)
}

// deploy-24 AC-1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_update_without_data_keeps_the_stored_data() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let create = json!({"op":"save_object","kind":"ac","id":"b","revision":0,
        "parent":"requirement:r","data":{"title":"B","owner":"owner","body":"First body"},
        "relations":{"related":{"edges":[{"to":"ac:a"}]}},"reason":"Create"});
    ok(&w, create).await?;
    ok(
        &w,
        update(1, json!({"data":{"body":"Given B, then B holds."}})),
    )
    .await?;
    let q = json!({"op":"save_object","kind":"requirement","id":"q","revision":0,"parent":null,
        "data":{"title":"Q","owner":"owner","body":"Another rule"},"reason":"Create"});
    ok(&w, q).await?;
    let before = w.object("ac:b").await?;
    ensure!(
        before["revision"] == 2 && edges(&before, "related") == [to("ac:a")],
        "{before}"
    );
    let data = before["data"].clone();
    ensure!(
        data["title"] == "B"
            && data["owner"] == "owner"
            && data["body"] == "Given B, then B holds.",
        "{before}"
    );
    // The first save removes the edges and keeps the data.
    let first = ok(&w, update(2, json!({"relations":{"related":null}}))).await?;
    ensure!(first["object"]["revision"] == 3, "{first}");
    let read = kept(&w, 3, "requirement:r", &data).await?;
    let left = relations(&read);
    ensure!(
        edges(&read, "related").is_empty()
            && left
                .as_object()
                .is_none_or(|m| m.values().all(|r| r["edges"] == json!([]))),
        "no edges: {read}"
    );
    // The second save moves ac:b and keeps the data.
    let second = ok(&w, update(3, json!({"parent":"requirement:q"}))).await?;
    ensure!(second["object"]["revision"] == 4, "{second}");
    kept(&w, 4, "requirement:q", &data).await?;
    // clean: false without data changes nothing.
    ok(&w, update(4, json!({"clean":false}))).await?;
    let (revision, _, document) = stored(&w, "ac:b").await?;
    ensure!(
        revision == 4 && document["data"] == data,
        "clean false adds no revision: {document}"
    );
    // clean: true and a create still need data. Each refusal stores nothing.
    let end = head(&w).await?;
    let clean = save(&w, update(4, json!({"clean":true}))).await?;
    refused(clean, "missing field `data`")?;
    let create = json!({"op":"save_object","kind":"ac","id":"c","revision":0,
        "parent":"requirement:r","reason":"Create without data"});
    refused(save(&w, create).await?, "missing field `data`")?;
    ensure!(head(&w).await? == end, "a refused save appends no event");
    ensure!(
        !exists(&w, "ac:c").await?,
        "the refused create stores no ac:c"
    );
    let (revision, _, document) = stored(&w, "ac:b").await?;
    ensure!(
        revision == 4 && document["data"] == data,
        "the refused clean save keeps ac:b: {document}"
    );
    Ok(())
}
