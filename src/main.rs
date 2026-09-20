use anyhow::{Result, bail};
use joppa::{commands::Command, store};
use serde_json::{Value, json};
use std::io::{Read, Write};
#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
mod legacy;
mod client;
mod client_config;
mod google_auth;
mod protocol;
mod web;
mod web_auth;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "legacy") {
        return legacy::run().await;
    }
    let client_command = args.first().is_none_or(|s| {
        matches!(
            s.as_str(),
            "mcp" | "list" | "read" | "command" | "login" | "logout" | "whoami"
        )
    });
    if client_command
        && (std::env::var_os("JOPPA_DATABASE_URL").is_none()
            || args
                .first()
                .is_some_and(|s| matches!(s.as_str(), "login" | "logout" | "whoami")))
    {
        return client::run(&args).await;
    }
    let pool = store::pool(
        &std::env::var("JOPPA_DATABASE_URL").unwrap_or_else(|_| store::DEFAULT_URL.into()),
    )?;
    match args.first().map(String::as_str) {
        Some("migrate") => {
            store::migrate(&pool).await?;
            println!("journal schema ready; legacy tables untouched");
        }
        Some("serve") => {
            let port = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(6212);
            let host = args.get(2).map(String::as_str).unwrap_or("127.0.0.1");
            web::serve(pool, host, port).await?;
        }
        Some("command") => {
            let workspace = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("command WORKSPACE REQUEST_ID < command.json"))?;
            let request_id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("request id required for safe retries"))?;
            let actor = actor()?;
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let command: Command = serde_json::from_str(&input)?;
            println!(
                "{}",
                store::execute(&pool, workspace, &actor, request_id, &command).await?
            );
        }
        Some("gate") => {
            let workspace = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("gate WORKSPACE < subject.json"))?;
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let subject = serde_json::from_str(&input)?;
            let card = store::read(&pool, workspace, &actor()?).await?;
            store::gate(&card, &subject)?;
            println!("explicit launch conditions accepted for this subject");
        }
        Some("read") => {
            let workspace = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("read WORKSPACE"))?;
            println!("{}", store::read(&pool, workspace, &actor()?).await?);
        }
        Some("list") => println!("{}", store::list(&pool, &actor()?).await?),
        Some("board") => {
            let workspace = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("board WORKSPACE OUTPUT.html"))?;
            let path = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("output path required"))?;
            let data = store::read(&pool, workspace, &actor()?).await?;
            std::fs::write(path, web::snapshot(&data))?;
            println!("{path}");
        }
        None | Some("mcp") => {
            let actor = actor()?;
            let stdin = std::io::stdin();
            for line in stdin.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let response = match serde_json::from_str::<Value>(&line) {
                    Ok(msg) => protocol::handle(&pool, &actor, &msg).await,
                    Err(e) => Some(
                        json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}}),
                    ),
                };
                if let Some(response) = response {
                    println!("{response}");
                    std::io::stdout().flush()?;
                }
            }
        }
        _ => bail!(
            "commands: migrate | serve [PORT HOST] | command WORKSPACE REQUEST_ID | read WORKSPACE | gate WORKSPACE | list | board WORKSPACE FILE | mcp | legacy ..."
        ),
    }
    Ok(())
}
fn actor() -> Result<String> {
    let actor = std::env::var("JOPPA_ACTOR").map_err(|_| {
        anyhow::anyhow!(
            "set JOPPA_ACTOR; local CLI/MCP uses the identity assigned by its trusted launcher"
        )
    })?;
    joppa::commands::identifier(&actor)?;
    Ok(actor)
}
