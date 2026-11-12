//! The helpers of the tests of the tone blue (requirement:vqlbur46) over a real server: reads
//! in other workspaces, the HTTP reads of the agent bot, the check that a reply holds no grey,
//! and the old journal events that a live write refuses.
#![allow(dead_code)]
use super::record_world::World;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};

/// The test of criterion A in each world of these tests.
pub const T: &str = "tests/blue.rs > t";

/// The pairs of the colour rule: an older saved pair and the pair of the tone blue.
pub const OLD_PAIRS: [(&str, &str); 2] = [("#b8b3a8", "#b4b8bf"), ("#c8c3b8", "#8a8f96")];
pub const NEW_PAIR: (&str, &str) = ("#6fa8dc", "#8bbbe8");

/// One MCP tool call of `token` in workspace `ws`. A refusal gives Err with the error text.
pub async fn mcp_in(
    w: &World,
    token: &str,
    ws: &str,
    tool: &str,
    mut args: Value,
) -> Result<Result<Value, String>> {
    args["workspace"] = json!(ws);
    let (code, reply) = w.mcp_raw(Some(token), tool, args).await?;
    ensure!(code == 200, "{tool}: {code} {reply}");
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .with_context(|| format!("MCP reply: {reply}"))?;
    Ok(if reply["result"]["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text)?)
    })
}

/// One GET of `token` below /api/workspaces/<W>, such as "/objects?address=ac:a".
pub async fn get(w: &World, token: &str, path: &str) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}{path}", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(token).send().await?;
    let code = reply.status();
    let body: Value = reply.json().await?;
    ensure!(code.is_success(), "GET {path}: {code} {body}");
    Ok(body)
}

/// The paths in `value` of each string value or object key "grey".
pub fn greys(value: &Value) -> Vec<String> {
    fn walk(value: &Value, path: &str, out: &mut Vec<String>) {
        match value {
            Value::String(s) if s == "grey" => out.push(path.to_owned()),
            Value::Array(items) => {
                for (i, v) in items.iter().enumerate() {
                    walk(v, &format!("{path}[{i}]"), out);
                }
            }
            Value::Object(map) => {
                for (k, v) in map {
                    if k == "grey" {
                        out.push(format!("{path}.{k} (key)"));
                    }
                    walk(v, &format!("{path}.{k}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, "", &mut out);
    out
}

/// Refuse a reply that holds the tone grey.
pub fn no_grey(what: &str, value: &Value) -> Result<()> {
    let found = greys(value);
    ensure!(found.is_empty(), "{what} gives grey at {found:?}: {value}");
    Ok(())
}

/// The address of the test node of A.
pub async fn test_node(w: &World) -> Result<String> {
    let page = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"ac:a","kind":"test"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    let address = page["items"][0]["address"].as_str().context("test node")?;
    Ok(address.to_owned())
}

/// A new workspace of the owner with `title`. The agent bot is a member, alex is not.
pub async fn workspace(w: &World, title: &str) -> Result<String> {
    let ws = format!("blue-{}", uuid::Uuid::new_v4().simple());
    let op = json!({"op":"workspace","title":title});
    store::execute_input(&w.pool, &ws, "owner", &format!("{ws}-1"), &op).await?;
    store::enroll_service(&w.pool, &ws, "owner", "bot", "agent").await?;
    Ok(ws)
}

/// Append one set_tree_schema event of an older release with the keys of `change` to the
/// journal of `ws`. A live write refuses the tone grey, so the event goes into the journal
/// directly. The server must be stopped: backfill_board replays the journal. Gives the schema
/// revision that the event makes.
pub async fn old_schema(w: &World, ws: &str, change: Value) -> Result<u64> {
    let tree = store::tree_schema(&w.pool, ws, "owner", None).await?;
    let revision = tree["schema"]["revision"].as_u64().context("revision")?;
    let mut command =
        json!({"op":"set_tree_schema","revision":revision,"reason":"Older status list"});
    for (k, v) in change.as_object().into_iter().flatten() {
        command[k] = v.clone();
    }
    let sql = "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,COALESCE(MAX(position),0)+1,'owner',$2,$3::text::jsonb FROM joppa.events WHERE workspace=$1";
    let request = uuid::Uuid::new_v4().to_string();
    let conn = w.pool.get().await?;
    conn.execute(sql, &[&ws, &request, &command.to_string()])
        .await?;
    store::backfill_board(&w.pool).await?;
    Ok(revision + 1)
}

/// The statuses of the tree of `ws` (at schema revision `revision`) as the owner reads it in
/// GET /api/workspaces/{ws}/tree. The MCP joppa_tree gives no statuses (ac:bwc9qvme).
pub async fn statuses(w: &World, ws: &str, revision: Option<u64>) -> Result<Value> {
    let query = revision.map_or(String::new(), |r| format!("?revision={r}"));
    let url = format!("{}/api/workspaces/{ws}/tree{query}", w.root);
    let reply = w.http.get(url).bearer_auth(&w.owner).send().await?;
    let code = reply.status();
    let tree: Value = reply.json().await?;
    ensure!(code.is_success(), "GET tree of {ws}{query}: {code} {tree}");
    no_grey(&format!("GET tree of {ws}"), &tree)?;
    Ok(tree["schema"]["statuses"].clone())
}

/// The one entry of the tone blue in a status list.
pub fn blue_entry(statuses: &Value) -> Result<Value> {
    let list = statuses.as_array().context("statuses")?;
    let blue: Vec<&Value> = list.iter().filter(|s| s["tone"] == "blue").collect();
    ensure!(blue.len() == 1, "one blue entry: {statuses}");
    Ok(blue[0].clone())
}

/// A status entry of the tone blue.
pub fn blue(light: &str, dark: &str) -> Value {
    json!({"tone":"blue","light":light,"dark":dark})
}
