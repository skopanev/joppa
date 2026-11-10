//! ac:4j9iir7x over a real server: a relation and an edge keep a palette name or a #rrggbb
//! colour (in lower case), and a save refuses any other colour. The checks read A and C over
//! MCP and over HTTP.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{change, declare, refused, relations, save, saved, stored};
use serde_json::{Value, json};

const REFUSAL: &str = "color is blue, purple, teal, amber, pink, indigo, cyan, lime or #rrggbb";

/// One HTTP request of the bot: a GET of `path` under the workspace, or a POST of `body` to it.
async fn http(w: &World, path: &str, body: Option<Value>) -> Result<(u16, String)> {
    let url = format!("{}/api/workspaces/{}/{path}", w.root, w.ws);
    let request = match body {
        Some(body) => w.http.post(url).json(&body),
        None => w.http.get(url),
    };
    let reply = request.bearer_auth(&w.bot).send().await?;
    Ok((reply.status().as_u16(), reply.text().await?))
}

/// The relations of `address` in the MCP read and in the HTTP read.
async fn reads(w: &World, address: &str) -> Result<[(&'static str, Value); 2]> {
    let mcp = relations(&w.object(address).await?);
    let (code, text) = http(w, &format!("objects?address={address}"), None).await?;
    ensure!(code == 200, "HTTP read of {address}: {code} {text}");
    let document: Value = serde_json::from_str(&text)?;
    Ok([("MCP", mcp), ("HTTP", relations(&document))])
}

/// The key `key` of `value` is the string `want`, or `value` has no key `key` when `want` is
/// None.
fn has(value: &Value, key: &str, want: Option<&str>) -> bool {
    match want {
        Some(want) => value.get(key) == Some(&json!(want)),
        None => value.get(key).is_none(),
    }
}

/// Both reads of `address` give swap with the colour `relation` and one edge to B with the
/// colour `edge`.
async fn colours(
    w: &World,
    address: &str,
    relation: Option<&str>,
    edge: Option<&str>,
    step: &str,
) -> Result<()> {
    for (name, relations) in reads(w, address).await? {
        let swap = &relations["swap"];
        let first = &swap["edges"][0];
        ensure!(
            swap.is_object()
                && has(swap, "color", relation)
                && first["to"] == "doc:b"
                && has(first, "color", edge),
            "{step}: the {name} read of {address} gives {relations}"
        );
    }
    Ok(())
}

/// The relations value with swap of colour `relation` and one edge to B of colour `edge`.
fn swap(relation: Value, edge: Value) -> Value {
    json!({"swap":{"color":relation,"edges":[{"to":"doc:b","color":edge}]}})
}

// ac:4j9iir7x
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn relation_and_edge_colours_are_kept_and_other_colours_refused() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    for address in ["doc:b", "doc:c"] {
        saved(&w, address, 0, json!({})).await?;
    }
    let first = swap(json!("teal"), json!("#AA0000"));
    saved(&w, "doc:a", 0, json!({"relations":first})).await?;
    colours(&w, "doc:a", Some("teal"), Some("#aa0000"), "the first save").await?;
    let second = swap(json!("#1A2B3C"), json!("#AA0000"));
    saved(&w, "doc:a", 1, json!({"relations":second})).await?;
    let kept = (Some("#1a2b3c"), Some("#aa0000"));
    colours(&w, "doc:a", kept.0, kept.1, "the second save").await?;
    // Each other relation colour, then one other edge colour, is refused.
    let mut bad: Vec<Value> = ["red", "Teal", "teal ", "#12345"]
        .into_iter()
        .map(|c| swap(json!(c), json!("#AA0000")))
        .collect();
    bad.push(swap(json!(7), json!("#AA0000")));
    bad.push(swap(json!("#1A2B3C"), json!("pink ")));
    for relations in bad {
        let reply = save(&w, "doc:a", 2, json!({"relations":relations})).await?;
        refused(reply, REFUSAL).map_err(|e| e.context(relations.to_string()))?;
    }
    // The same refusal over HTTP.
    let command = json!({"op":"save_object","kind":"doc","id":"a","revision":2,"data":{},
        "relations":swap(json!("red"), json!("#AA0000")),"reason":"Colour over HTTP"});
    let body = json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command});
    let (code, text) = http(&w, "commands", Some(body)).await?;
    ensure!(
        (400..500).contains(&code) && text.contains(REFUSAL),
        "HTTP save of red: {code} {text}"
    );
    let revision = stored(&w, "doc:a").await?.0;
    ensure!(revision == 2, "the refusals store no revision: {revision}");
    colours(&w, "doc:a", kept.0, kept.1, "after the refusals").await?;
    change(&w, "doc:a", json!({"body":"Only new data"})).await?;
    colours(&w, "doc:a", kept.0, kept.1, "the save with only new data").await?;
    // A relation and an edge saved without a colour read without the key color.
    let plain = json!({"swap":{"edges":[{"to":"doc:b"}]}});
    change(&w, "doc:c", json!({"relations":plain})).await?;
    colours(&w, "doc:c", None, None, "the save of C").await?;
    // A save of the read relations of A back is accepted and changes no relation.
    let read = relations(&w.object("doc:a").await?);
    change(&w, "doc:a", json!({"relations":read})).await?;
    let again = relations(&w.object("doc:a").await?);
    ensure!(
        again == read,
        "the read relations saved back: {read} then {again}"
    );
    colours(&w, "doc:a", kept.0, kept.1, "the save of the read back").await
}
