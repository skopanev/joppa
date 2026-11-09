//! requirement:file-upload over a real server: joppa_attach with upload true gives a signed
//! upload link on Joppa. One PUT to the link stores the bytes and writes the attach. A bad,
//! late, empty or large upload, or an upload after the member left, stores nothing.
#[path = "common/attach_url.rs"]
mod attach_url;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use attach_url::{OVER, attach, files, link, put, refusal, sha};
use record_world::fake_spaces;
use serde_json::{Value, json};

const FORM: &str = "application/x-www-form-urlencoded";

/// The object keys of `value`, in order.
fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// `token` with one changed character near its middle.
fn changed(token: &str) -> String {
    let mut chars: Vec<char> = token.chars().collect();
    let at = (chars.len() / 2..chars.len())
        .find(|&i| chars[i].is_ascii_alphanumeric())
        .unwrap_or(0);
    chars[at] = if chars[at] == 'A' { 'B' } else { 'A' };
    chars.into_iter().collect()
}

// ac:34wr41bv
// test:1ozo04iw
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn attach_with_upload_gives_a_15_minute_link_and_stores_nothing_yet() -> Result<()> {
    let w = record_world::world_env(&[], &[]).await?;
    let spaces = fake_spaces::start().keys();
    let before = chrono::Utc::now().timestamp();
    let fields = json!({"content_type":"application/pdf","check_result":"pass","request_id":"r2"});
    let first = link(&w, "ac:a", fields).await?;
    let after = chrono::Utc::now().timestamp();
    ensure!(
        keys(&first) == ["expires_at", "target", "upload_url", "workspace"]
            && first["workspace"] == w.ws.as_str()
            && first["target"] == "ac:a",
        "the first reply: {first}"
    );
    let url = first["upload_url"].as_str().context("upload_url")?;
    let token = url.strip_prefix(&format!("{}/api/uploads/", w.root));
    ensure!(
        token.is_some_and(|t| !t.is_empty() && !t.contains(['/', '?', '#'])),
        "upload_url is not {}/api/uploads/<token>: {url}",
        w.root
    );
    let expires = first["expires_at"].as_str().context("expires_at")?;
    let at = chrono::DateTime::parse_from_rfc3339(expires)?;
    ensure!(
        at.offset().local_minus_utc() == 0
            && (before + 900..=after + 900).contains(&at.timestamp()),
        "expires_at {expires} is not 900 s after the call ({before}..{after}) in UTC"
    );
    ensure!(files(&w, &w.bot, "ac:a").await?.is_empty(), "A has a file");
    let one = "send exactly one of content, content_base64, url or upload";
    let base = json!({"filename":"b.pdf","upload":true});
    let mut on_r = base.clone();
    on_r["target"] = json!("requirement:r");
    on_r["check_result"] = json!("pass");
    let mut with_content = base.clone();
    with_content["target"] = json!("ac:a");
    with_content["content"] = json!("x");
    let mut outsider = base.clone();
    outsider["target"] = json!("ac:a");
    let mut failures = Vec::new();
    for (step, token, args, text) in [
        (
            "on R",
            &w.bot,
            on_r,
            "a manual check belongs to a criterion",
        ),
        ("with content", &w.bot, with_content, one),
        ("outsider", &w.outsider, outsider, "Workspace not found"),
    ] {
        let reply = attach(&w, token, args).await?;
        failures.extend(refusal(step, &reply, text));
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    let list = files(&w, &w.bot, "ac:a").await?;
    ensure!(list.is_empty(), "A has files: {list:?}");
    ensure!(fake_spaces::start().keys() == spaces, "Spaces has new keys");
    Ok(())
}

// ac:hb4nerzr
// test:m15tnj03
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_put_to_the_upload_link_stores_the_file_and_writes_the_attach() -> Result<()> {
    let w = record_world::world_env(&[], &[]).await?;
    let fake = fake_spaces::start();
    let fields = json!({"alias":"p1","content_type":"application/pdf","check_result":"pass"});
    let link = link(&w, "ac:a", fields).await?;
    let url = link["upload_url"].as_str().context("upload_url")?;
    let seed = uuid::Uuid::new_v4();
    let bytes: Vec<u8> = seed.as_bytes().iter().cycle().take(3000).copied().collect();
    let hash = sha(&bytes);
    let (code, first) = put(url, bytes.clone(), FORM).await?;
    let file = &first["file"];
    ensure!(
        code == 200
            && keys(&first) == ["file", "position", "replayed", "target", "workspace"]
            && first["workspace"] == w.ws.as_str()
            && first["target"] == "ac:a"
            && first["position"].is_u64()
            && first["replayed"] == false,
        "the first PUT: {code} {first}"
    );
    ensure!(
        file["name"] == "b.pdf"
            && file["content_type"] == "application/pdf"
            && file["size"] == 3000
            && file["hash"] == hash.as_str()
            && file["actor"] == "bot"
            && file["alias"] == "p1"
            && file["check_result"] == "pass"
            && file["url"].as_str().is_some_and(|u| u.contains(&hash)),
        "the file of the first PUT: {file}"
    );
    ensure!(
        fake.bytes(&w.ws, &hash).as_ref() == Some(&bytes),
        "Spaces has no such bytes under workspaces/{}/{hash}",
        w.ws
    );
    let (code, again) = put(url, bytes.clone(), FORM).await?;
    ensure!(
        code == 200 && again["replayed"] == true && again["file"]["id"] == file["id"],
        "the second PUT: {code} {again}"
    );
    let other: Vec<u8> = bytes.iter().map(|b| b ^ 0xff).collect();
    let (code, third) = put(url, other.clone(), FORM).await?;
    ensure!(
        code == 409 && third == json!({"error":"request_id reused with different content/actor"}),
        "the third PUT: {code} {third}"
    );
    ensure!(
        !fake.any(&sha(&other)),
        "Spaces has the bytes of the third PUT"
    );
    let list = files(&w, &w.bot, "ac:a").await?;
    let such: Vec<&Value> = list.iter().filter(|f| f["hash"] == hash.as_str()).collect();
    ensure!(
        list.len() == 1 && such.len() == 1 && such[0]["id"] == file["id"],
        "A lists {list:?}"
    );
    Ok(())
}

// ac:cxcb5yaa
// test:ubfnp84u
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_upload_link_refuses_a_bad_late_or_empty_upload_and_stores_nothing() -> Result<()> {
    let w = record_world::world_env(&[], &[]).await?;
    let spaces = fake_spaces::start().keys();
    let short = [("JOPPA_UPLOAD_LINK_SECONDS", "1".to_owned())];
    let (_second, second_root) = w.spare(&short).await?;
    let url = |reply: &Value| reply["upload_url"].as_str().map(str::to_owned);
    let fields = || json!({"filename":"f.bin"});
    let (bad, empty, large, left) = (
        url(&link(&w, "ac:a", fields()).await?).context("bad")?,
        url(&link(&w, "ac:a", fields()).await?).context("empty")?,
        url(&link(&w, "ac:a", fields()).await?).context("large")?,
        url(&link(&w, "ac:a", fields()).await?).context("left")?,
    );
    // The second server issues the late link. Its JOPPA_PUBLIC_URL is its own root.
    let call = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_attach",
        "arguments":{"workspace":w.ws,"target":"ac:a","filename":"f.bin","upload":true}}});
    let reply = w
        .http
        .post(format!("{second_root}/mcp"))
        .bearer_auth(&w.bot)
        .json(&call);
    let reply: Value = reply.send().await?.json().await?;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap_or("");
    let late = serde_json::from_str::<Value>(text)
        .ok()
        .as_ref()
        .and_then(url);
    let late = late.with_context(|| format!("no link from the second server: {reply}"))?;
    ensure!(
        late.starts_with(&format!("{second_root}/api/uploads/")),
        "{late}"
    );
    let (base, token) = bad.rsplit_once('/').context("token")?;
    let bad = format!("{base}/{}", changed(token));
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    // Each body would also fail a later check: the order of the checks gives the reply.
    let error = |text: &str| json!({ "error": text });
    let mut failures = Vec::new();
    let mut send = async |step: &str, url: &str, len: usize, want: u16, text: &str| {
        match put(url, vec![7u8; len], FORM).await {
            Ok((code, body)) if code == want && body == error(text) => {}
            got => failures.push(format!("{step}: want {want} {}, got {got:?}", error(text))),
        }
        anyhow::Ok(())
    };
    send(
        "changed token",
        &bad,
        60 * 1024 * 1024,
        403,
        "upload link is not valid",
    )
    .await?;
    send("late", &late, OVER, 410, "upload link expired").await?;
    send("empty", &empty, 0, 409, "an empty file carries nothing").await?;
    send(
        "large",
        &large,
        OVER,
        413,
        "an attachment is at most 50 MiB",
    )
    .await?;
    let remove = json!({"op":"remove_member","id":"bot","reason":"Left the team"});
    w.command(&w.owner, remove)
        .await?
        .map_err(anyhow::Error::msg)?;
    send("left", &left, OVER, 404, "Workspace not found").await?;
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    let list = files(&w, &w.owner, "ac:a").await?;
    ensure!(list.is_empty(), "A has files: {list:?}");
    let now = fake_spaces::start().keys();
    ensure!(
        now == spaces,
        "Spaces has new keys: before {spaces:?}, after {now:?}"
    );
    Ok(())
}
