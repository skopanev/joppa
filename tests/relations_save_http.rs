//! requirement:q438uw0s over a real server: save_object merges relations by key and checks the
//! relation keys, the edge targets and the pins.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{
    change, declare, edges, last_event, msg, refused, relations, save, saved, stored, to,
};
use serde_json::{Value, json};

/// The stored relations of `address`: the current document in joppa.objects.
async fn document(w: &World, address: &str) -> Result<Value> {
    Ok(relations(&json!({"document":stored(w, address).await?.2})))
}

/// The key `key` of `address` holds edges [] in the document and in the last journal event.
async fn empty_key(w: &World, address: &str, key: &str, title: Option<&str>) -> Result<()> {
    let stored = document(w, address).await?;
    let event = last_event(w).await?;
    for (name, relations) in [("document", &stored), ("journal", &event["relations"])] {
        let entry = &relations[key];
        ensure!(entry["edges"] == json!([]), "{name}: {relations}");
        if let Some(title) = title {
            ensure!(entry["title"] == title, "{name}: {relations}");
        }
    }
    Ok(())
}

// ac:84v38a55
// test:x4inukfn
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn save_object_merges_relations_by_key() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap"), ("swap-money", "Swap money")]).await?;
    for address in ["doc:b", "doc:c", "doc:e"] {
        saved(&w, address, 0, json!({})).await?;
    }
    let first = json!({"swap":{"title":"Enter amount","edges":[{"to":"doc:b","when":"limit ok"}]},
        "swap-money":{"edges":[{"to":"doc:c"}]}});
    saved(&w, "doc:a", 0, json!({"relations":first})).await?;
    let both = async |step: &str| -> Result<()> {
        let read = w.object("doc:a").await?;
        let swap = &relations(&read)["swap"];
        ensure!(
            swap["title"] == "Enter amount"
                && edges(&read, "swap") == [to("doc:b")]
                && swap["edges"][0]["when"] == "limit ok"
                && edges(&read, "swap-money") == [to("doc:c")],
            "{step}: {read}"
        );
        Ok(())
    };
    both("the first save").await?;
    change(&w, "doc:a", json!({"body":"Only data"})).await?;
    both("the data save").await?;
    let clean = json!({"clean":true,"title":"a","owner":"owner","body":"Clean data"});
    change(&w, "doc:a", clean).await?;
    both("the clean save").await?;
    change(
        &w,
        "doc:a",
        json!({"relations":{},"body":"Empty relations"}),
    )
    .await?;
    both("the save of relations {}").await?;
    change(
        &w,
        "doc:a",
        json!({"relations":{"swap":{"edges":[{"to":"doc:c"}]}}}),
    )
    .await?;
    let read = w.object("doc:a").await?;
    let title = relations(&read)["swap"]["title"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    ensure!(
        title.is_empty()
            && edges(&read, "swap") == [to("doc:c")]
            && edges(&read, "swap-money") == [to("doc:c")],
        "the swap key is replaced and swap-money stays: {read}"
    );
    change(&w, "doc:a", json!({"relations":{"swap-money":null}})).await?;
    let read = w.object("doc:a").await?;
    let left = relations(&read);
    ensure!(
        left.get("swap-money").is_none() && edges(&read, "swap") == [to("doc:c")],
        "{read}"
    );
    let (revision, ..) = stored(&w, "doc:a").await?;
    let null = save(&w, "doc:a", revision, json!({"relations":null})).await?;
    refused(null, "relations must be an object")?;
    // A key without edges stores edges [], with and without a title.
    saved(&w, "doc:e", 1, json!({"relations":{"swap":{}}})).await?;
    empty_key(&w, "doc:e", "swap", None).await?;
    saved(
        &w,
        "doc:e",
        2,
        json!({"relations":{"swap":{"title":"End"}}}),
    )
    .await?;
    empty_key(&w, "doc:e", "swap", Some("End")).await
}

// ac:xo4b5he5
// test:fw821p22
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn save_object_checks_relation_keys_targets_and_pins() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    for address in ["doc:b", "doc:y", "doc:z"] {
        saved(&w, address, 0, json!({})).await?;
    }
    let archive = async |address: &str| -> Result<()> {
        let (revision, ..) = stored(&w, address).await?;
        let command = json!({"op":"archive_object","target":address,"revision":revision,
            "reason":"Done"});
        w.command(&w.bot, command).await?.map_err(msg)?;
        Ok(())
    };
    archive("doc:z").await?;
    let y = json!({"related":{"edges":[{"to":"doc:y"}]}});
    saved(&w, "doc:a", 0, json!({"relations":y})).await?;
    archive("doc:y").await?;
    for (relations, error) in [
        (json!({"nope":{}}), "unknown link relation"),
        (
            json!({"swap":{"edges":[{"to":"doc:z"}]}}),
            "linked object is absent from the current tree",
        ),
        (
            json!({"swap":{"edges":[{"to":"doc:b"},{"to":"doc:b"}]}}),
            "duplicate object link",
        ),
        (
            json!({"swap":{"edges":[{"to":"doc:b","revision":99}]}}),
            "unknown linked revision",
        ),
    ] {
        let reply = save(&w, "doc:a", 1, json!({"relations":relations})).await?;
        refused(reply, error)?;
    }
    let kept = json!({"related":{"edges":[{"to":"doc:y"},{"to":"doc:b"}]}});
    saved(&w, "doc:a", 1, json!({"relations":kept})).await?;
    let read = w.object("doc:a").await?;
    ensure!(
        edges(&read, "related") == [to("doc:y"), to("doc:b")],
        "{read}"
    );
    let y = &relations(&read)["related"]["edges"][0];
    ensure!(y["present"] == false, "the edge to the archived Y: {read}");
    Ok(())
}
