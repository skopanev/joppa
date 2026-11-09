//! The helpers of the relation tests (requirement:q438uw0s) over a real server (record_world.rs).
//! The world adds the types doc (document), task, note (inbox) and src (source) to the record
//! world. Saves go through MCP joppa_command. An old journal gets its events in the database
//! while the server is stopped, and the server replays it at the next start.
#![allow(dead_code)]
use super::record_world::{self, World};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// An MCP refusal text as an error.
pub fn msg(e: String) -> anyhow::Error {
    anyhow::Error::msg(e)
}

/// A node type under `parents` with `extra` keys, such as status or fields.
pub fn node(title: &str, role: &str, parents: &[&str], extra: Value) -> Value {
    let mut t = json!({"title":title,"role":role,"status":"none","root":parents.is_empty(),
        "parents":parents});
    for (k, v) in extra.as_object().into_iter().flatten() {
        t[k] = v.clone();
    }
    t
}

/// The record world with the criterion tests `tests` and the types doc, task, note and src.
pub async fn world(tests: &[&str]) -> Result<World> {
    let w = record_world::world(tests).await?;
    let types = json!({"doc":node("Documents", "document", &[], json!({})),
        "task":node("Tasks", "task", &["ac", "requirement"], json!({})),
        "note":node("Notes", "document", &[], json!({"inbox":true})),
        "src":node("Sources", "document", &[], json!({"source":true}))});
    schema(&w, json!({"put_types":types})).await?.map_err(msg)?;
    Ok(w)
}

/// The tree of the workspace, or of its schema revision `revision`, as `token` reads it.
pub async fn tree(w: &World, token: &str, revision: Option<u64>) -> Result<Value> {
    let args = revision.map_or(json!({}), |r| json!({"revision":r}));
    w.mcp(token, "joppa_tree", args).await?.map_err(msg)
}

/// The relation registry of a tree reply: schema.relations, or relations of the reply.
pub fn registry(tree: &Value) -> Value {
    let schema = &tree["schema"]["relations"];
    if schema.is_object() {
        schema.clone()
    } else {
        tree["relations"].clone()
    }
}

/// One set_tree_schema of the owner at the current revision with the keys of `change`.
pub async fn schema(w: &World, change: Value) -> Result<Result<Value, String>> {
    let revision = tree(w, &w.owner, None).await?["schema"]["revision"].clone();
    let mut command =
        json!({"op":"set_tree_schema","revision":revision,"reason":"Configure the relations"});
    for (k, v) in change.as_object().into_iter().flatten() {
        command[k] = v.clone();
    }
    w.command(&w.owner, command).await
}

/// Declare the flow relations `put` (key to title) as the owner.
pub async fn declare(w: &World, put: &[(&str, &str)]) -> Result<()> {
    let put: serde_json::Map<String, Value> = put
        .iter()
        .map(|(k, t)| ((*k).to_owned(), json!({"title":t})))
        .collect();
    schema(w, json!({"put_relations":put}))
        .await?
        .map_err(msg)?;
    Ok(())
}

/// One save_object of `token` for `address` at `revision`. The keys parent, relations, links
/// and clean of `extra` go to the command, each other key goes to data. A new node gets the
/// title of its ID and the owner owner.
pub async fn save_as(
    w: &World,
    token: &str,
    address: &str,
    revision: u64,
    extra: Value,
) -> Result<Result<Value, String>> {
    let (kind, id) = address.split_once(':').context("address")?;
    let data = if revision == 0 {
        json!({"title":id,"owner":"owner","body":"Relation fixture","source":"test"})
    } else {
        json!({})
    };
    let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "data":data,"reason":"Relation test"});
    if revision == 0 && kind != "note" {
        command["data"].as_object_mut().map(|d| d.remove("source"));
    }
    for (k, v) in extra.as_object().into_iter().flatten() {
        match k.as_str() {
            "parent" | "relations" | "links" | "clean" => command[k] = v.clone(),
            _ => command["data"][k] = v.clone(),
        }
    }
    w.command(token, command).await
}

/// save_as of the agent bot.
pub async fn save(
    w: &World,
    address: &str,
    revision: u64,
    extra: Value,
) -> Result<Result<Value, String>> {
    save_as(w, &w.bot, address, revision, extra).await
}

/// A save of the agent bot that must succeed.
pub async fn saved(w: &World, address: &str, revision: u64, extra: Value) -> Result<Value> {
    save(w, address, revision, extra)
        .await?
        .map_err(|e| anyhow::anyhow!("{address}: {e}"))
}

/// A save of the bot at the current revision of `address` that must succeed.
pub async fn change(w: &World, address: &str, extra: Value) -> Result<Value> {
    let revision = stored(w, address).await?.0;
    saved(w, address, revision, extra).await
}

/// The stored revision, presence and document of `address`.
pub async fn stored(w: &World, address: &str) -> Result<(u64, bool, Value)> {
    let sql = "SELECT revision::bigint,present,document::text FROM joppa.objects WHERE workspace=$1 AND address=$2";
    let row = w
        .pool
        .get()
        .await?
        .query_one(sql, &[&w.ws, &address])
        .await?;
    let revision: i64 = row.get(0);
    Ok((
        revision as u64,
        row.get(1),
        serde_json::from_str(row.get(2))?,
    ))
}

/// The command of the last journal event of the workspace.
pub async fn last_event(w: &World) -> Result<Value> {
    let sql =
        "SELECT command::text FROM joppa.events WHERE workspace=$1 ORDER BY position DESC LIMIT 1";
    let row = w.pool.get().await?.query_one(sql, &[&w.ws]).await?;
    Ok(serde_json::from_str(row.get(0))?)
}

/// The relations of a read: relations of an MCP detail or of an HTTP document.
pub fn relations(read: &Value) -> Value {
    match read.get("document") {
        Some(document) => document["relations"].clone(),
        None => read["relations"].clone(),
    }
}

/// The edges of the relation `key` of a read as (to, revision) pairs, in their order.
pub fn edges(read: &Value, key: &str) -> Vec<(String, Option<u64>)> {
    relations(read)[key]["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            (
                e["to"].as_str().unwrap_or("").to_owned(),
                e["revision"].as_u64(),
            )
        })
        .collect()
}

/// The (to, revision) pair of an edge without a pin.
pub fn to(address: &str) -> (String, Option<u64>) {
    (address.to_owned(), None)
}

/// A refusal that contains `error`.
pub fn refused(reply: Result<Value, String>, error: &str) -> Result<()> {
    ensure!(
        reply.as_ref().is_err_and(|e| e.contains(error)),
        "refusal with {error:?} expected: {reply:?}"
    );
    Ok(())
}

/// A JSON value has no object key `key` at any depth.
pub fn without_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => !map.contains_key(key) && map.values().all(|v| without_key(v, key)),
        Value::Array(items) => items.iter().all(|v| without_key(v, key)),
        _ => true,
    }
}

/// The tone of `address` in joppa_objects.
pub async fn tone(w: &World, address: &str) -> Result<Value> {
    Ok(w.object(address).await?["status"]["tone"].clone())
}

/// The stored criterion boundaries of the workspace: address and time.
pub async fn boundaries(w: &World) -> Result<Vec<(String, String)>> {
    let sql = "SELECT key,value#>>'{}' FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries' ORDER BY key";
    let rows = w.pool.get().await?.query(sql, &[&w.ws]).await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// Append the events `commands` of an old journal as the owner while the server is stopped.
/// The relations_reset epoch moves after them and after `later` old events still to come, as
/// migration 062 stores it. The server replays the journal at its next start. Gives the
/// position and the time of each event.
pub async fn old_journal(w: &World, commands: &[Value], later: i64) -> Result<Vec<(i64, String)>> {
    let conn = w.pool.get().await?;
    let mut out = Vec::new();
    for command in commands {
        let sql = "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,COALESCE(MAX(position),0)+1,'owner',$2,$3::text::jsonb FROM joppa.events WHERE workspace=$1 RETURNING position,recorded_at::text";
        let request = uuid::Uuid::new_v4().to_string();
        let row = conn
            .query_one(sql, &[&w.ws, &request, &command.to_string()])
            .await?;
        out.push((row.get(0), row.get(1)));
    }
    let epoch = "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) SELECT $1,'relations_reset',MAX(position)+1+$2 FROM joppa.events WHERE workspace=$1 ON CONFLICT (workspace,rule) DO UPDATE SET first_position=EXCLUDED.first_position";
    conn.execute(epoch, &[&w.ws, &later]).await?;
    Ok(out)
}

/// The old stored form of one link: depends_on without a pin stores no revision field.
pub fn link(target: &str, relation: &str, blocking: bool, revision: Option<u64>) -> Value {
    let mut l =
        json!({"target":target,"relation":relation,"blocking":blocking,"revision":revision});
    if relation == "depends_on" && revision.is_none() {
        l.as_object_mut().map(|m| m.remove("revision"));
    }
    l
}

/// An old save_object event of `address` at `revision` with `data` and the old `links`.
pub fn old_save(address: &str, revision: u64, parent: Value, data: Value, links: Value) -> Value {
    let (kind, id) = address.split_once(':').unwrap_or_default();
    json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,
        "data":data,"links":links,"reason":format!("Old save {revision}")})
}
