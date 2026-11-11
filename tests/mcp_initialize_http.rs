//! requirement:mcp-tools over a real server: initialize gives the transport, read and write
//! rules in at most 3500 bytes. joppa_help gives the writing rules and the process rules in
//! its sections writing and process.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// The texts that send an agent to joppa_search before a read, or to a read after a write.
const HARMFUL: [&str; 3] = [
    "Resolve a copied public ID",
    "joppa_search({",
    "Read the returned revision after the write",
];

// ac:6022ynjr
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn initialize_gives_the_read_and_write_rules_in_at_most_3500_bytes() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let text = reply["result"]["instructions"]
        .as_str()
        .context("instructions")?;
    let bytes = text.len();
    ensure!(
        bytes <= 3500,
        "the instructions have {bytes} bytes, more than 3500"
    );
    for kept in [
        "next_cursor",
        "fields",
        "2 to 25 addresses",
        "revision",
        "relations",
        "Evidence",
        "alias",
        "Over 1 kb: prefer attach as a file",
    ] {
        ensure!(text.contains(kept), "the instructions do not hold {kept:?}");
    }
    for moved in [
        "11111",
        "Reviewer",
        "Test-5",
        "ASD-STE100 Issue 9",
        "Resolve a copied public ID",
        "joppa_search({",
    ] {
        ensure!(!text.contains(moved), "the instructions hold {moved:?}");
    }
    // One sentence says that joppa_objects reads a public item ID as an address.
    let sentence = text
        .split(['\n', '.'])
        .any(|s| s.contains("joppa_objects") && s.contains("public item ID"));
    ensure!(
        sentence,
        "no sentence names joppa_objects and a public item ID"
    );
    let section = async |name: &str| -> Result<String> {
        let reply = w.mcp(&w.bot, "joppa_help", json!({"section":name})).await?;
        let reply = reply.map_err(anyhow::Error::msg)?;
        let text = reply[name]
            .as_str()
            .with_context(|| format!("{name}: {reply}"))?;
        Ok(text.to_owned())
    };
    let writing = section("writing").await?;
    ensure!(
        writing == include_str!("../docs/writing.md"),
        "section writing is not docs/writing.md whole"
    );
    let process = section("process").await?;
    for rule in ["11111", "Reviewer", "Test-5"] {
        ensure!(process.contains(rule), "section process has no {rule:?}");
    }
    let objects = section("objects").await?;
    for (name, text) in [
        ("writing", &writing),
        ("process", &process),
        ("objects", &objects),
    ] {
        for phrase in HARMFUL {
            ensure!(!text.contains(phrase), "section {name} holds {phrase:?}");
        }
    }
    Ok(())
}
