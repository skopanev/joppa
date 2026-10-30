//! requirement:search-all-versions and requirement:search-all-workspaces over a real server:
//! the HTTP route, the MCP tool joppa_search and the CLI command search give the same items
//! for each flag value.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn db() -> Result<(deadpool_postgres::Pool, String)> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok((pool, url))
}

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<()> {
    store::execute_input(
        pool,
        ws,
        "owner",
        &uuid::Uuid::new_v4().to_string(),
        &command,
    )
    .await?;
    Ok(())
}

async fn workspace(pool: &deadpool_postgres::Pool, member: bool) -> Result<String> {
    let ws = format!("scope-{}", uuid::Uuid::new_v4().simple());
    write(pool, &ws, json!({"op":"workspace","title":"Scope"})).await?;
    if member {
        store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    }
    Ok(ws)
}

fn save(id: &str, title: &str, revision: u32) -> Value {
    json!({"op":"save_object","kind":"domain","id":id,"revision":revision,"parent":null,"data":{"title":title,"owner":"owner"},"reason":"Scope test text"})
}

/// The same search over HTTP, MCP and the CLI. Gives the items when the three agree.
async fn search(
    root: &str,
    ws: &str,
    query: &str,
    versions: bool,
    workspaces: bool,
) -> Result<Vec<Value>> {
    let args = [
        ("q", query),
        ("limit", "10"),
        ("all_versions", &versions.to_string()),
        ("all_workspaces", &workspaces.to_string()),
    ];
    // The indexer embeds the rows after the server starts. The three reads compare one
    // ranking, so they start when every row has its vector.
    let mut http = world::http_search(root, "bot", ws, &args).await?;
    for _ in 0..100 {
        if http["semantic_status"] == "ready" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        http = world::http_search(root, "bot", ws, &args).await?;
    }
    ensure!(http["semantic_status"] == "ready", "{http}");
    let tool = world::mcp(
        root,
        "bot",
        "joppa_search",
        json!({"workspace":ws,"query":query,"all_versions":versions,"all_workspaces":workspaces}),
    )
    .await?;
    let token = std::env::temp_dir().join(format!("joppa-cli-{}", uuid::Uuid::new_v4()));
    std::fs::write(&token, world::token("bot"))?;
    let mut cli = std::process::Command::new("python3");
    cli.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/client/joppa"))
        .args([
            "search",
            query,
            "--workspace",
            ws,
            "--url",
            &format!("{root}/mcp"),
            "--token-file",
        ])
        .arg(&token);
    if versions {
        cli.arg("--all-versions");
    }
    if workspaces {
        cli.arg("--all-workspaces");
    }
    // The test provider runs on this runtime, so the CLI waits on a blocking thread.
    let out = tokio::task::spawn_blocking(move || cli.output()).await??;
    ensure!(
        out.status.success(),
        "CLI: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cli: Value = serde_json::from_slice(&out.stdout).context("CLI output")?;
    // MCP and the CLI give the HTTP items without empty keys, item IDs and public IDs,
    // without `req` on a requirement, with `workspace` only for all workspaces, with the
    // address of each hit and without false flags.
    let mut compact = joppa::read_compact::strip_reply(http.clone());
    for item in compact["items"].as_array_mut().into_iter().flatten() {
        let map = item.as_object_mut().context("item")?;
        map.remove("system_id");
        if map.get("kind") == Some(&json!("requirement")) {
            map.remove("req");
        }
        if !workspaces {
            map.remove("workspace");
        }
    }
    let compact = joppa::mcp_lean::lean("joppa_search", &json!({}), compact);
    ensure!(
        compact["items"] == tool["items"] && tool["items"] == cli["items"],
        "HTTP, MCP and CLI disagree:\n{http}\n{tool}\n{cli}"
    );
    // A search of one workspace names it once, in the reply.
    let mut items = tool["items"].as_array().cloned().unwrap_or_default();
    for item in &mut items {
        if item.get("workspace").is_none() {
            item["workspace"] = json!(ws);
        }
    }
    Ok(items)
}

fn find<'a>(items: &'a [Value], ws: &str, id: &str) -> Vec<&'a Value> {
    items
        .iter()
        .filter(|i| i["workspace"] == ws && i["address"] == format!("domain:{id}").as_str())
        .collect()
}

// search-all-versions-on
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn with_all_versions_a_word_of_an_old_revision_finds_that_revision() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = workspace(&pool, true).await?;
    write(&pool, &ws, save("billing", "Legacy billing rules", 0)).await?;
    write(&pool, &ws, save("billing", "Billing rules", 1)).await?;
    write(&pool, &ws, save("export", "Legacy export", 0)).await?;
    write(&pool, &ws, json!({"op":"archive_object","target":"domain:export","revision":1,"reason":"Out of scope"})).await?;
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    let on = search(&server.root, &ws, "legacy", true, false).await?;
    let old = find(&on, &ws, "billing");
    ensure!(
        old.iter()
            .any(|i| i["revision"] == 1 && i["historical"] == true && i["archived"] != true),
        "{on:?}"
    );
    ensure!(
        find(&on, &ws, "export")
            .iter()
            .any(|i| i["archived"] == true),
        "{on:?}"
    );
    let off = search(&server.root, &ws, "legacy", false, false).await?;
    ensure!(
        off.iter()
            .all(|i| i["historical"] != true && i["archived"] != true),
        "{off:?}"
    );
    ensure!(find(&off, &ws, "export").is_empty(), "{off:?}");
    ensure!(
        find(&off, &ws, "billing")
            .iter()
            .all(|i| !i["title"].as_str().unwrap_or("").contains("Legacy")),
        "{off:?}"
    );
    Ok(())
}

// search-all-workspaces-on
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn with_all_workspaces_the_reader_finds_each_own_workspace_only() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let (a, b, c) = (
        workspace(&pool, true).await?,
        workspace(&pool, true).await?,
        workspace(&pool, false).await?,
    );
    for ws in [&a, &b, &c] {
        write(&pool, ws, save("keys", "Passkey sign in", 0)).await?;
    }
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    let on = search(&server.root, &a, "passkey", false, true).await?;
    ensure!(
        !find(&on, &a, "keys").is_empty() && !find(&on, &b, "keys").is_empty(),
        "{on:?}"
    );
    ensure!(
        on.iter().all(|i| i["workspace"] != c.as_str()),
        "a workspace without access: {on:?}"
    );
    let off = search(&server.root, &a, "passkey", false, false).await?;
    ensure!(
        off.iter().all(|i| i["workspace"] == a.as_str()) && !find(&off, &a, "keys").is_empty(),
        "{off:?}"
    );
    Ok(())
}
