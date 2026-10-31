//! requirement:read-contract over a real server: one joppa_objects read gives 1 to 10 objects,
//! `fields` names the keys of an object read by its address, and no reply is above 256 KB.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, args: Value) -> Result<Result<Value, String>> {
    w.mcp(&w.bot, "joppa_objects", args).await
}

async fn ok(w: &record_world::World, args: Value) -> Result<Value> {
    read(w, args).await?.map_err(anyhow::Error::msg)
}

async fn save(w: &record_world::World, id: &str, body: &str) -> Result<()> {
    let save = json!({"op":"save_object","kind":"requirement","id":id,"revision":0,
        "parent":null,"data":{"title":id,"owner":"owner","body":body},"reason":"Add"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    Ok(())
}

// read-many-addresses
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_read_gives_up_to_ten_objects_with_their_addresses() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    save(&w, "s", "Second rule").await?;
    let r = ok(&w, json!({"address":"requirement:r"})).await?;
    let a = ok(&w, json!({"address":"ac:a"})).await?;
    // One address gives the object itself; more addresses give items in the order of the request.
    let both = ok(&w, json!({"address":"ac:a, requirement:r"})).await?;
    let named = |mut item: Value, address: &str| {
        item["address"] = json!(address);
        item
    };
    ensure!(
        r.get("address").is_none()
            && both
                == json!({"items": [named(a.clone(), "ac:a"), named(r.clone(), "requirement:r")]}),
        "{both}"
    );
    let eleven = ["requirement:r"; 11].join(",");
    let refused = [
        (
            json!({"address":eleven}),
            "at most 10 addresses in one call",
        ),
        (
            json!({"address":"ac:a,"}),
            "empty address in the address list",
        ),
        (
            json!({"address":"ac:a,requirement:r","history":true}),
            "history, revision and comments_cursor read one address",
        ),
        (
            json!({"address":"ac:a,requirement:r","revision":1}),
            "history, revision and comments_cursor read one address",
        ),
        (json!({"address":"ac:a,nope:x"}), "unknown object"),
    ];
    for (args, error) in refused {
        let reply = read(&w, args.clone()).await?;
        ensure!(reply == Err(error.into()), "{args}: {reply:?}");
    }
    let ten = ["requirement:s"; 10].join(",");
    let ten = ok(&w, json!({"address":ten})).await?;
    ensure!(
        ten["items"].as_array().map(Vec::len) == Some(10)
            && ten["items"][9]["address"] == "requirement:s",
        "{ten}"
    );
    Ok(())
}

// read-address-fields
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn fields_name_the_keys_of_an_object_read_by_its_address() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let fields = json!(["title", "status", "data.owner", "parent", "kind"]);
    let one = ok(&w, json!({"address":"ac:a","fields":fields})).await?;
    let list = ok(&w, json!({"parent":"requirement:r","fields":fields})).await?;
    let mut item = list["items"][0].clone();
    item.as_object_mut().map(|m| m.remove("address"));
    let detail = ok(&w, json!({"address":"ac:a"})).await?;
    item["revision"] = detail["revision"].clone();
    // The values are the values of the list item, with the revision that a write sends.
    ensure!(
        one == item && one["data.owner"] == "owner" && one["parent"] == "requirement:r",
        "{one} {item}"
    );
    let both = ok(
        &w,
        json!({"address":"ac:a,requirement:r","fields":["title"]}),
    )
    .await?;
    let r = ok(&w, json!({"address":"requirement:r"})).await?;
    ensure!(
        both == json!({"items": [
            {"address":"ac:a","revision":detail["revision"],"title":"A"},
            {"address":"requirement:r","revision":r["revision"],"title":"R"}]}),
        "{both}"
    );
    for args in [
        json!({"address":"ac:a","fields":["nope"]}),
        json!({"address":"ac:a","fields":["title"],"revision":1}),
        json!({"address":"ac:a","fields":["title"],"history":true}),
    ] {
        let reply = read(&w, args.clone()).await?;
        ensure!(
            reply == Err("unknown field nope".into())
                || reply == Err("fields reads the current revision".into()),
            "{args}: {reply:?}"
        );
    }
    Ok(())
}

// read-reply-ceiling
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_reply_above_256_kb_is_refused() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let tree = w.mcp(&w.bot, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    let mut types = tree["schema"]["types"].clone();
    types["requirement"]["field_max_length"] = json!(200_000);
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "types":types,"reason":"Long texts"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    let body = "x".repeat(150_000);
    save(&w, "big1", &body).await?;
    save(&w, "big2", &body).await?;
    // Each object alone fits; the two together are above the ceiling, and fields narrow them.
    let one = ok(&w, json!({"address":"requirement:big1"})).await?;
    let both = read(&w, json!({"address":"requirement:big1,requirement:big2"})).await?;
    let narrow = json!({"address":"requirement:big1,requirement:big2","fields":["title"]});
    let narrow = ok(&w, narrow).await?;
    ensure!(
        one["data"]["body"] == json!(body)
            && both
                == Err("reply exceeds 256 KB; narrow the request with fields, fewer addresses or a smaller limit".into())
            && narrow["items"][1]["title"] == "big2",
        "{both:?} {narrow}"
    );
    Ok(())
}
