//! The board key of a typed field over a real server. Type ac has the field stage with board:
//! true and the field note without it. MCP joppa_tree gives no board key, GET /tree gives board
//! true only on stage, a copy of the MCP fields keeps it, and a board value that is not a
//! boolean is refused.
#[path = "common/computed_world.rs"]
mod computed_world;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use computed_world::{node, schema};
use record_world::{A, World};
use relations_world::{msg, refused, without_key};
use serde_json::{Value, json};

/// The criterion type with the field stage, board `board`, and the field note.
fn criteria(board: Value) -> Value {
    let stage = json!({"type":"enum","values":["plan","done"],"board":board});
    let fields = json!({"fields":{"stage":stage,"note":{"type":"text"}}});
    node("Criteria", "ac", &["requirement"], fields)
}

/// The fields of type ac in GET /tree as member alex reads it.
async fn member_fields(w: &World) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}/tree", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.alex).send().await?;
    let code = reply.status();
    let tree: Value = reply.json().await?;
    ensure!(code.is_success(), "GET tree: {code} {tree}");
    Ok(tree["schema"]["types"]["ac"]["fields"].clone())
}

/// GET /tree gives board true on stage and no board key on note.
async fn board_only_on_stage(w: &World, step: &str) -> Result<()> {
    let fields = member_fields(w).await?;
    let right = fields["stage"]["board"] == true
        && fields["note"].is_object()
        && fields["note"].get("board").is_none();
    ensure!(right, "{step}: GET /tree fields of ac: {fields}");
    Ok(())
}

// ac:sigd1mxp
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_typed_field_shows_on_the_board_card_only_with_board_true() -> Result<()> {
    let w = record_world::world(&[]).await?;
    schema(&w, json!({"ac":criteria(json!(true))}))
        .await?
        .map_err(msg)?;
    let revision = w.object(A).await?["revision"].clone();
    let save = json!({"op":"save_object","kind":"ac","id":"a","revision":revision,
        "data":{"stage":"plan","note":"x"},"reason":"Plan A"});
    w.command(&w.bot, save).await?.map_err(msg)?;
    let data = w.object(A).await?["data"].clone();
    ensure!(data["stage"] == "plan" && data["note"] == "x", "{data}");
    let tree = w.mcp(&w.bot, "joppa_tree", json!({})).await?.map_err(msg)?;
    ensure!(without_key(&tree, "board"), "MCP joppa_tree: {tree}");
    board_only_on_stage(&w, "first read").await?;
    // The agent sends type ac back with the fields that joppa_tree gave.
    let copy = tree["schema"]["types"]["ac"].clone();
    ensure!(copy["fields"]["stage"].is_object(), "{tree}");
    schema(&w, json!({"ac":copy})).await?.map_err(msg)?;
    board_only_on_stage(&w, "after the copy").await?;
    let yes = schema(&w, json!({"ac":criteria(json!("yes"))})).await?;
    refused(yes, "board")?;
    board_only_on_stage(&w, "after the refusal").await?;
    Ok(())
}
