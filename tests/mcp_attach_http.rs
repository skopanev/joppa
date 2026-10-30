//! requirement:file-upload over MCP: joppa_attach puts the bytes in Spaces and attaches
//! them. joppa_attachments and joppa_objects give each file a presigned download url.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use record_world::fake_spaces;
use serde_json::{Value, json};

fn ok(reply: Result<Value, String>) -> Result<Value> {
    reply.map_err(anyhow::Error::msg)
}

async fn attach(
    w: &record_world::World,
    token: &str,
    args: Value,
) -> Result<Result<Value, String>> {
    w.mcp(token, "joppa_attach", args).await
}

// file-attach-mcp
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn joppa_attach_stores_the_file_in_spaces_and_lists_it_with_a_download_url() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let fake = fake_spaces::start();
    let text = format!("MCP evidence {}", uuid::Uuid::new_v4());
    let hash = joppa::digest_hex::sha256_hex(&text);
    let call = json!({"target":"ac:a","filename":"evidence.txt","content_type":"text/plain",
        "content":text,"request_id":format!("attach-{hash}")});
    let first = ok(attach(&w, &w.bot, call.clone()).await?)?;
    let file = &first["file"];
    ensure!(
        file["hash"] == hash.as_str()
            && file["size"] == text.len()
            && file["name"] == "evidence.txt",
        "{first}"
    );
    ensure!(
        fake.bytes(&hash) == Some(text.clone().into_bytes()),
        "Spaces has other bytes"
    );
    // An identical retry replays the same attach and adds no file.
    let again = ok(attach(&w, &w.bot, call).await?)?;
    ensure!(
        again["replayed"] == true && again["file"]["id"] == file["id"],
        "{again}"
    );
    let binary: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    let second = ok(attach(
        &w,
        &w.bot,
        json!({"target":"ac:a","filename":"bytes.bin",
        "content_base64":STANDARD.encode(&binary)}),
    )
    .await?)?;
    ensure!(
        second["file"]["content_type"] == "application/octet-stream",
        "{second}"
    );
    // joppa_attachments lists the newest file first, and each url downloads byte-exact.
    let listed = ok(w
        .mcp(&w.bot, "joppa_attachments", json!({"target":"ac:a"}))
        .await?)?;
    let files = listed["files"].as_array().context("files")?;
    ensure!(
        files.len() == 2 && files[0]["name"] == "bytes.bin" && files[1]["name"] == "evidence.txt",
        "{listed}"
    );
    for (file, bytes, content_type) in [
        (&files[0], binary.clone(), "application/octet-stream"),
        (&files[1], text.clone().into_bytes(), "text/plain"),
    ] {
        let url = file["url"].as_str().context("url")?;
        let prefix = format!(
            "{}/{}/files/{}?",
            fake.endpoint,
            fake_spaces::BUCKET,
            file["hash"].as_str().unwrap()
        );
        ensure!(
            url.starts_with(&prefix) && url.contains("X-Amz-Expires=900"),
            "{url}"
        );
        let response = reqwest::get(url).await?;
        ensure!(response.status() == 200, "{}", response.status());
        ensure!(response.headers()[reqwest::header::CONTENT_TYPE] == content_type);
        ensure!(
            response.bytes().await?.as_ref() == bytes,
            "a download changed the bytes"
        );
    }
    // joppa_objects gives the same files with a url.
    let object = ok(w
        .mcp(&w.bot, "joppa_objects", json!({"address":"ac:a"}))
        .await?)?;
    let urls: Vec<&str> = object["files"]
        .as_array()
        .context("files")?
        .iter()
        .filter_map(|f| f["url"].as_str())
        .collect();
    ensure!(
        urls.len() == 2 && urls.iter().all(|u| u.contains("X-Amz-Signature=")),
        "{object}"
    );
    // Refused: both contents, no content, more than 50 MiB, a non-member and a read token.
    let refused = |reply: Result<Value, String>, text: &str| match reply {
        Err(e) if e.contains(text) => Ok(()),
        other => Err(anyhow::anyhow!("{text}: {other:?}")),
    };
    let base = json!({"target":"ac:a","filename":"x.txt"});
    let mut both = base.clone();
    both["content"] = json!("x");
    both["content_base64"] = json!("eA==");
    refused(
        attach(&w, &w.bot, both).await?,
        "exactly one of content or content_base64",
    )?;
    refused(
        attach(&w, &w.bot, base.clone()).await?,
        "exactly one of content or content_base64",
    )?;
    let over = vec![9u8; joppa::spaces::ATTACHMENT_MAX + 1];
    let mut large = base.clone();
    large["content_base64"] = json!(STANDARD.encode(&over));
    refused(attach(&w, &w.bot, large).await?, "at most 50 MiB")?;
    ensure!(
        !fake.has(&joppa::digest_hex::sha256_hex(&over)),
        "the large file is in Spaces"
    );
    let secret = format!("Outsider file {}", uuid::Uuid::new_v4());
    let mut outsider = base.clone();
    outsider["content"] = json!(secret);
    ensure!(
        attach(&w, &w.outsider, outsider.clone()).await?.is_err(),
        "a non-member attached"
    );
    let token =
        joppa::store::create_agent_token(&w.pool, &w.ws, "owner", "file-reader", Some("read"))
            .await?;
    let reader = token["token"].as_str().context("token")?;
    refused(attach(&w, reader, outsider).await?, "read")?;
    ensure!(
        !fake.has(&joppa::digest_hex::sha256_hex(&secret)),
        "a refused attach stored the file"
    );
    let read = ok(w
        .mcp(reader, "joppa_attachments", json!({"target":"ac:a"}))
        .await?)?;
    ensure!(read["files"].as_array().map_or(0, Vec::len) == 2, "{read}");
    Ok(())
}
