//! The test schema and the tree of the POST /api/data walk tests (record_world.rs). The names
//! box, type_a, type_b, env, area, retries, strict, tags and lane are test schema names.
#![allow(dead_code)]
use super::read_key::command;
use super::record_world::World;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// A test type under `parents` with the typed fields `fields`. A type without parents is a root
/// type.
pub fn node(title: &str, status: &str, parents: &[&str], fields: Value) -> Value {
    json!({"title":title,"role":"document","status":status,"root":parents.is_empty(),
        "parents":parents,"fields":fields})
}

/// The enum field env with the values dev and prod.
pub fn env() -> Value {
    json!({"type":"enum","values":["dev","prod"]})
}

/// Put the types of `put` with set_tree_schema, as the owner.
pub async fn put_types(w: &World, put: Value) -> Result<()> {
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let schema = json!({"op":"set_tree_schema","revision":revision,"put_types":put,
        "reason":"Configure the test schema"});
    command(w, &w.owner, schema).await?;
    Ok(())
}

/// Save a new node at `address` under `parent` with the data `data` and the title of its id.
pub async fn save(w: &World, address: &str, parent: Option<&str>, data: Value) -> Result<()> {
    let (kind, id) = address.split_once(':').context("address")?;
    let mut all = json!({"title": id});
    for (k, v) in data.as_object().into_iter().flatten() {
        all[k] = v.clone();
    }
    let save = json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
        "data":all,"reason":"Build the test tree"});
    command(w, &w.bot, save).await?;
    Ok(())
}

/// Archive the node at `address` at its current revision.
pub async fn archive(w: &World, address: &str) -> Result<()> {
    let revision = w.object(address).await?["revision"].clone();
    ensure!(revision.is_u64(), "{address}: revision {revision}");
    let archive = json!({"op":"archive_object","target":address,"revision":revision,
        "reason":"Close the branch"});
    command(w, &w.bot, archive).await?;
    Ok(())
}

/// The tree of the walk tests. box:root has type_a:a3 (no env), type_a:a1 (prod) and type_a:a2
/// (dev) in this tree order, which a reorder sets. a3 has type_b:b3, a1 has type_b:b1 (prod)
/// and a2 has type_b:b2 (prod). type_a:a4 (prod) and its child type_b:b4 are archived.
/// box:other has type_a:c1 (prod). The titles of a1, b2, a4 and c1 hold "zebrafish".
/// An enum field refuses "", so b3 has no env value: the walk reads both as unrestricted.
/// With `quagga`, box:root also has 12 live type_b nodes with "quagga" in the body.
pub async fn walk_tree(w: &World, quagga: bool) -> Result<()> {
    let fields = json!({"env": env()});
    put_types(
        w,
        json!({
            "box": node("Boxes", "children", &[], json!({})),
            "type_a": node("Type A", "own", &["box"], fields.clone()),
            "type_b": node("Type B", "own", &["type_a", "box"], fields),
        }),
    )
    .await?;
    let prod = || json!({"env":"prod"});
    save(w, "box:root", None, json!({})).await?;
    save(w, "box:other", None, json!({})).await?;
    let root = Some("box:root");
    save(
        w,
        "type_a:a1",
        root,
        json!({"env":"prod","title":"a1 zebrafish"}),
    )
    .await?;
    save(w, "type_a:a2", root, json!({"env":"dev"})).await?;
    save(w, "type_a:a3", root, json!({})).await?;
    let order = json!({"op":"reorder_tree","kind":"type_a","parent":"box:root",
        "expected":["a1","a2","a3"],"ids":["a3","a1","a2"]});
    command(w, &w.bot, order).await?;
    save(w, "type_b:b3", Some("type_a:a3"), json!({})).await?;
    save(w, "type_b:b1", Some("type_a:a1"), prod()).await?;
    let b2 = json!({"env":"prod","title":"b2 zebrafish"});
    save(w, "type_b:b2", Some("type_a:a2"), b2).await?;
    let a4 = json!({"env":"prod","title":"a4 zebrafish"});
    save(w, "type_a:a4", root, a4).await?;
    save(w, "type_b:b4", Some("type_a:a4"), prod()).await?;
    // A node with a live child cannot be archived, so the child goes first.
    archive(w, "type_b:b4").await?;
    archive(w, "type_a:a4").await?;
    let c1 = json!({"env":"prod","title":"c1 zebrafish"});
    save(w, "type_a:c1", Some("box:other"), c1).await?;
    let count = if quagga { 12 } else { 0 };
    for i in 0..count {
        // Each node holds the word a different number of times, so the ranks differ.
        let body = format!("{} grazes here", "quagga ".repeat(i + 1));
        let data = json!({"title": format!("Animal {i}"), "body": body});
        save(w, &format!("type_b:q{i:02}"), root, data).await?;
    }
    Ok(())
}
