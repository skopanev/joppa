//! requirement:bleqltl4 over a real server: a tree type names the type of its data fields,
//! a save refuses a value of another type, and the board offers the values of an enum field.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use record_world::World;
use serde_json::{Value, json};

/// Put the type ac with `fields`, as the owner. Gives the refusal text or the reply.
async fn put_fields(w: &World, fields: Value) -> Result<Result<Value, String>> {
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let mut ac = tree["schema"]["types"]["ac"].clone();
    ac["fields"] = fields;
    let put = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "put_types":{"ac":ac},"filters":["env"],"reason":"Type the fields"});
    w.command(&w.owner, put).await
}

/// A save of a new criterion with `extra` data under requirement:r.
async fn save(w: &World, id: &str, extra: Value) -> Result<Result<Value, String>> {
    let mut data = json!({"title":id,"owner":"owner"});
    for (k, v) in extra.as_object().into_iter().flatten() {
        data[k] = v.clone();
    }
    let command = json!({"op":"save_object","kind":"ac","id":id,"revision":0,
        "parent":"requirement:r","data":data,"reason":"Typed save"});
    w.command(&w.bot, command).await
}

// ac:115mquwp
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_typed_field_refuses_a_value_of_another_type_and_the_board_offers_its_values()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    // A bad rule is refused.
    for (fields, error) in [
        (
            json!({"env":{"type":"enum"}}),
            "env: an enum field needs values",
        ),
        (
            json!({"due":{"type":"date"}}),
            "due: a field type is text, number, boolean, enum or list",
        ),
        (
            json!({"size":{"type":"number","values":["1"]}}),
            "size: only an enum or a list field has values",
        ),
        (json!({"title":{"type":"text"}}), "not title or body: title"),
    ] {
        let reply = put_fields(&w, fields.clone()).await?;
        ensure!(
            reply.as_ref().is_err_and(|e| e.contains(error)),
            "{fields}: {reply:?}"
        );
    }
    // A stored value that breaks a new rule refuses the rule.
    save(&w, "old", json!({"env":"qa"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let fields = json!({"env":{"type":"enum","values":["dev","prod"]},"urgent":{"type":"boolean"},
        "size":{"type":"number"},"tags":{"type":"list"},"note":{"type":"text"}});
    let reply = put_fields(&w, fields.clone()).await?;
    ensure!(
        reply
            .as_ref()
            .is_err_and(|e| e.contains("env is one of dev, prod")),
        "{reply:?}"
    );
    let old = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"ac:old"}))
        .await?;
    let revision = old.map_err(anyhow::Error::msg)?["revision"].clone();
    let fix = json!({"op":"save_object","kind":"ac","id":"old","revision":revision,"data":{"env":"dev"},"reason":"Fix env"});
    w.command(&w.bot, fix).await?.map_err(anyhow::Error::msg)?;
    put_fields(&w, fields).await?.map_err(anyhow::Error::msg)?;
    for (extra, error) in [
        (json!({"env":"qa"}), "env is one of dev, prod"),
        (json!({"urgent":"yes"}), "urgent is true or false"),
        (json!({"size":"3"}), "size is a number"),
        (json!({"tags":"a"}), "tags is a list of texts"),
        (json!({"note":7}), "note is a text"),
    ] {
        let reply = save(&w, "bad", extra.clone()).await?;
        ensure!(
            reply.as_ref().is_err_and(|e| e.contains(error)),
            "{extra}: {reply:?}"
        );
    }
    let good = json!({"env":"prod","urgent":true,"size":3,"tags":["a"],"note":"n"});
    save(&w, "good", good).await?.map_err(anyhow::Error::msg)?;
    // The env drop-down offers both values, also dev that no node names now.
    let url = format!("{}/api/workspaces/{}/board", w.root, w.ws);
    let board: Value = w
        .http
        .get(url)
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    let wanted = json!([{"field":"env","values":[{"value":"dev","list":false},{"value":"prod","list":false}]}]);
    ensure!(board["filters"] == wanted, "{}", board["filters"]);
    Ok(())
}
