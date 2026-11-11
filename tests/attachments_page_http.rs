//! requirement:read-contract over a real server: joppa_attachments pages the files of an object
//! by limit and cursor, and refuses a reply above 262144 bytes. The fields read of joppa_objects
//! gives the short files.
#[path = "common/http_command.rs"]
mod http_command;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

const CEILING: &str =
    "reply exceeds 256 KB; narrow the request with fields, fewer addresses or a smaller limit";

/// 300 files on ac:a from 300 attach ops, newest last. MCP hides the attach op, so each op goes
/// through POST /api/workspaces/W/commands. Each file has a name of about 250 characters and the
/// hash of one stored file.
async fn files(w: &record_world::World, hash: &Value, size: &Value) -> Result<()> {
    let long = "evidence-".repeat(27);
    for i in 0..300 {
        let op = json!({"op":"attach","target":"ac:a","id":format!("f{i:03}"),
            "name":format!("{long}{i:03}.txt"),"content_type":"text/plain","size":size,
            "hash":hash});
        http_command::ok(w, &w.bot, op).await?;
    }
    Ok(())
}

/// The time of a file: `at`, else `created_at`.
fn at(file: &Value) -> String {
    let at = if file["at"].is_null() {
        &file["created_at"]
    } else {
        &file["at"]
    };
    at.as_str().unwrap_or_default().to_owned()
}

// ac:jn3hjrnu
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn joppa_attachments_pages_files_and_refuses_a_reply_above_256_kb() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let save = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
        "data":{"title":"B","owner":"owner"},"reason":"Create"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    let mut stored = Value::Null;
    for name in ["one.txt", "two.txt"] {
        let file = json!({"target":"ac:b","filename":name,"content":format!("Evidence {name}")});
        stored = w
            .mcp(&w.bot, "joppa_attach", file)
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    files(&w, &stored["file"]["hash"], &stored["file"]["size"]).await?;
    let world = &w;
    let list = |args: Value| async move { world.mcp(&world.bot, "joppa_attachments", args).await };
    // All 300 files are above the ceiling. The 2 files of ac:b come.
    let all = list(json!({"target":"ac:a"})).await?;
    ensure!(all == Err(CEILING.into()), "{all:?}");
    let b = list(json!({"target":"ac:b"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(b["files"].as_array().map(Vec::len) == Some(2), "{b}");
    // limit 100 and each cursor give 3 pages of 100, newest first, each file once.
    let (mut seen, mut cursor, mut sizes) = (Vec::<Value>::new(), Value::Null, vec![]);
    loop {
        let mut args = json!({"target":"ac:a","limit":100});
        if !cursor.is_null() {
            args["cursor"] = cursor.clone();
        }
        let page = list(args).await?.map_err(anyhow::Error::msg)?;
        let files = page["files"].as_array().cloned().context("files")?;
        sizes.push(files.len());
        seen.extend(files);
        cursor = page.get("next_cursor").cloned().unwrap_or(Value::Null);
        if cursor.is_null() || sizes.len() > 3 {
            break;
        }
    }
    let mut ids: Vec<&str> = seen.iter().filter_map(|f| f["id"].as_str()).collect();
    let times: Vec<String> = seen.iter().map(at).collect();
    ensure!(
        sizes == [100, 100, 100] && times.windows(2).all(|t| t[0] >= t[1]),
        "{sizes:?} {times:?}"
    );
    ids.sort_unstable();
    ids.dedup();
    ensure!(ids.len() == 300, "{} distinct files", ids.len());
    for limit in [0, 101] {
        let reply = list(json!({"target":"ac:a","limit":limit})).await?;
        ensure!(reply.is_err(), "limit {limit}: {reply:?}");
    }
    // The short files of joppa_objects stay under the ceiling.
    let named = json!({"workspace":w.ws,"address":"ac:a","fields":["files"]});
    let (code, reply) = w.mcp_raw(Some(&w.bot), "joppa_objects", named).await?;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap_or("");
    let named: Value = serde_json::from_str(text)?;
    ensure!(
        code == 200
            && reply["result"]["isError"] != true
            && text.len() < 262_144
            && named["files"].as_array().map(Vec::len) == Some(300),
        "{} bytes: {reply}",
        text.len()
    );
    Ok(())
}

// ac:jn3hjrnu
#[test]
fn a_reply_text_with_a_files_list_above_256_kb_is_refused() {
    let file = json!({"name":"x".repeat(1000)});
    let files = json!({"files": vec![file.clone(); 300]});
    let items = json!({"items": vec![file; 300]});
    for reply in [files, items] {
        let refused = joppa::mcp_lean::text(&reply).map_err(|e| e.to_string());
        assert_eq!(refused, Err(CEILING.to_owned()));
    }
    // One object without such a list still comes in full.
    let one = json!({"data":{"body":"x".repeat(300_000)}});
    assert!(joppa::mcp_lean::text(&one).is_ok_and(|t| t.len() > 262_144));
    let small = json!({"files":[{"name":"a.txt"}]});
    assert!(joppa::mcp_lean::text(&small).is_ok());
}
