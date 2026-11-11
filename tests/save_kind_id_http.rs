//! requirement:item-id-code over a real server: the id of save_object takes the kind:id form.
//! The journal stores the local id. An id of another kind or a bad form is refused.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Result, ensure};
use save_world::{events_after, head, item_id, ok, refused, save};
use serde_json::{Value, json};

/// A save_object of `kind` with the id `id` at `revision` with `data`.
fn save_of(kind: &str, id: &str, revision: u64, data: Value) -> Value {
    let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "data":data,"reason":"Save by kind:id"});
    if revision == 0 {
        command["parent"] = json!("requirement:r");
    }
    command
}

// deploy-24 AC-2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_id_of_save_object_takes_the_kind_id_form() -> Result<()> {
    let w = record_world::world(&[]).await?;
    ok(
        &w,
        save_of("ac", "x", 0, json!({"title":"X","owner":"owner"})),
    )
    .await?;
    let x = item_id(&w, "ac", "x").await?;
    let start = head(&w).await?;
    let first = ok(
        &w,
        save_of("ac", "ac:x", 1, json!({"title":"X by address"})),
    )
    .await?;
    ensure!(
        first["object"]["address"] == "ac:x" && first["object"]["revision"] == 2,
        "{first}"
    );
    let fresh = json!({"title":"Fresh","owner":"owner"});
    let created = ok(&w, save_of("ac", "ac:fresh", 0, fresh)).await?;
    ensure!(created["object"]["address"] == "ac:fresh", "{created}");
    let read = w.object("ac:x").await?;
    ensure!(
        read["revision"] == 2 && read["data"]["title"] == "X by address",
        "{read}"
    );
    let read = w.object("ac:fresh").await?;
    ensure!(
        read["data"]["title"] == "Fresh" && read["parent"] == "requirement:r",
        "{read}"
    );
    // The journal events name the local ids, never the kind:id form.
    let events = events_after(&w, start).await?;
    let ids: Vec<&Value> = events
        .iter()
        .filter(|e| e["op"] == "save_object")
        .map(|e| &e["id"])
        .collect();
    ensure!(ids == [&json!("x"), &json!("fresh")], "{events:?}");
    // An id of another kind and a bad kind:id form are refused and store nothing.
    let end = head(&w).await?;
    let rule = json!({"title":"Wrong","owner":"owner","body":"A rule"});
    let wrong = save(&w, save_of("requirement", "ac:x", 2, rule)).await?;
    refused(wrong, "the id names an object of type ac, not requirement")?;
    for id in ["ac:", "ac:x:y"] {
        let data = json!({"title":"Bad","owner":"owner"});
        let reply = save(&w, save_of("ac", id, 0, data)).await?;
        refused(reply, "invalid object id")?;
    }
    ensure!(head(&w).await? == end, "no refused save appends an event");
    // A public item ID still names the object.
    let by_item = ok(&w, save_of("ac", &x, 2, json!({"title":"X by item ID"}))).await?;
    ensure!(
        by_item["object"]["address"] == "ac:x" && by_item["object"]["revision"] == 3,
        "{by_item}"
    );
    let read = w.object("ac:x").await?;
    ensure!(
        read["revision"] == 3 && read["data"]["title"] == "X by item ID",
        "{read}"
    );
    Ok(())
}
