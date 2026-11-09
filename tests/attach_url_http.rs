//! requirement:file-upload over a real server: joppa_attach with url fetches the bytes from the
//! test origin O, stores them in Spaces and attaches them. A private address, plain http, too
//! many redirects, a large or missing file and a slow reply store nothing.
#[path = "common/attach_url.rs"]
mod attach_url;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use attach_url::{attach, files, ok, origin, origin_env, refusal, sha};
use record_world::fake_spaces;
use serde_json::{Value, json};

/// The file ids of a list of files, in order.
fn ids(files: &[Value]) -> Vec<String> {
    let mut ids: Vec<String> = files
        .iter()
        .filter_map(|f| f["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids
}

/// Each line of `failures` is one refused step that went wrong. A has no file, and Spaces has
/// the keys `keys`.
async fn nothing_stored(
    w: &record_world::World,
    keys: &[String],
    failures: Vec<String>,
) -> Result<()> {
    let list = files(w, &w.bot, "ac:a").await?;
    let now = fake_spaces::start().keys();
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    ensure!(list.is_empty(), "A has files: {list:?}");
    ensure!(
        now == keys,
        "Spaces has new keys: before {keys:?}, after {now:?}"
    );
    Ok(())
}

// ac:a8aay7d5
// test:4rly7epe
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn attach_with_url_stores_the_bytes_of_the_reply() -> Result<()> {
    let w = record_world::world_env(&[], &origin_env()).await?;
    let (o, fake) = (origin(), fake_spaces::start());
    let hash = sha(&o.a);
    let a_txt = format!("{}/a.txt", o.root);
    let call = json!({"target":"ac:a","filename":"a.txt","url":a_txt,"request_id":"r1"});
    let first = ok(attach(&w, &w.bot, call.clone()).await?)?;
    let file = &first["file"];
    ensure!(
        file["hash"] == hash.as_str()
            && file["size"] == 2048
            && file["content_type"] == "text/plain; charset=utf-8",
        "the first call: {first}"
    );
    ensure!(
        fake.bytes(&w.ws, &hash).as_ref() == Some(&o.a),
        "Spaces has no such bytes under workspaces/{}/{hash}: {:?}",
        w.ws,
        fake.keys()
    );
    let again = ok(attach(&w, &w.bot, call).await?)?;
    ensure!(
        again["replayed"] == true && again["file"]["id"] == file["id"],
        "the repeat: {again}"
    );
    let hops = json!({"target":"ac:a","filename":"a.txt","url":format!("{}/three-hops", o.root)});
    let hops = ok(attach(&w, &w.bot, hops).await?)?;
    ensure!(
        hops["file"]["hash"] == hash.as_str(),
        "the three-hop call: {hops}"
    );
    let markdown =
        json!({"target":"ac:a","filename":"a.txt","content_type":"text/markdown","url":a_txt});
    let markdown = ok(attach(&w, &w.bot, markdown).await?)?;
    let md = &markdown["file"];
    ensure!(
        md["content_type"] == "text/markdown"
            && md["hash"] == hash.as_str()
            && md["id"] != file["id"],
        "the call with content_type: {markdown}"
    );
    // joppa_attachments of A gives each file of the calls, and no other file.
    let list = files(&w, &w.bot, "ac:a").await?;
    let made: Vec<Value> = [file, &hops["file"], md].into_iter().cloned().collect();
    let mut want = ids(&made);
    want.dedup();
    ensure!(
        ids(&list) == want,
        "A lists {list:?}, the calls gave {made:?}"
    );
    let listed = list.iter().find(|f| f["id"] == md["id"]).context("md")?;
    ensure!(listed["content_type"] == "text/markdown", "{listed}");
    Ok(())
}

// ac:l83lfswu
// test:04whsv0u
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn attach_refuses_a_url_with_a_private_address_and_stores_nothing() -> Result<()> {
    let w = record_world::world_env(&[], &origin_env()).await?;
    let keys = fake_spaces::start().keys();
    let hosts = [
        "127.0.0.1:1",
        "0.0.0.0",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "224.0.0.1",
        "[::1]",
        "[::]",
        "[fd00::1]",
        "[fe80::1]",
        "[::ffff:127.0.0.1]",
        // A name: localhost has only private addresses.
        "localhost:1",
    ];
    let mut failures = Vec::new();
    for host in hosts {
        let url = format!("https://{host}/a");
        let call = json!({"target":"ac:a","filename":"a.txt","url":url});
        let reply = attach(&w, &w.bot, call).await?;
        failures.extend(refusal(&url, &reply, "url resolves to a private address"));
    }
    nothing_stored(&w, &keys, failures).await
}

// ac:ffibh2rq
// test:lz5y1nsd
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn attach_refuses_a_bad_url_fetch_and_stores_nothing() -> Result<()> {
    let w = record_world::world_env(&[], &origin_env()).await?;
    let (o, keys) = (origin(), fake_spaces::start().keys());
    let at = |path: &str| format!("{}{path}", o.root);
    let cases = [
        ("http://example.com/a".to_owned(), "url must use https"),
        (at("/redirect-private"), "url resolves to a private address"),
        (at("/redirect-http"), "url must use https"),
        (at("/redirect-loop"), "url has more than 3 redirects"),
        (at("/big-length"), "an attachment is at most 50 MiB"),
        (at("/big-chunked"), "an attachment is at most 50 MiB"),
        (at("/missing"), "url gave HTTP 404"),
        (at("/slow"), "url did not finish in 30 seconds"),
    ];
    let mut failures = Vec::new();
    for (url, text) in cases {
        let call = json!({"target":"ac:a","filename":"a.txt","url":url});
        let started = std::time::Instant::now();
        let reply = attach(&w, &w.bot, call).await?;
        failures.extend(refusal(&url, &reply, text));
        // The 30 s cover the whole fetch: the slow case ends soon after them.
        let took = started.elapsed().as_secs();
        if url.ends_with("/slow") && !(29..=35).contains(&took) {
            failures.push(format!("{url}: the refusal came after {took} s"));
        }
    }
    let both = json!({"target":"ac:a","filename":"a.txt","content":"x","url":at("/a.txt")});
    let reply = attach(&w, &w.bot, both).await?;
    let one = "send exactly one of content, content_base64, url or upload";
    failures.extend(refusal("content and url", &reply, one));
    nothing_stored(&w, &keys, failures).await
}
