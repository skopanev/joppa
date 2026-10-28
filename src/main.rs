use anyhow::{Result, bail};
use joppa::{commands::Command, store};
use std::io::Read;
mod google_auth;
mod protocol;
mod web;
mod web_auth;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pool = store::pool(
        &std::env::var("JOPPA_DATABASE_URL").unwrap_or_else(|_| store::DEFAULT_URL.into()),
    )?;
    match args.first().map(String::as_str) {
        Some("check-journal") => {
            store::check_journal(&pool).await?;
            println!("journal is compatible");
        }
        Some("migrate") => {
            store::migrate(&pool).await?;
            println!("journal schema ready; legacy tables untouched");
        }
        Some("backfill-board") => {
            println!(
                "board projections created: {}",
                store::backfill_board(&pool).await?
            );
        }
        Some("serve") => {
            let port = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(6212);
            let host = args.get(2).map(String::as_str).unwrap_or("127.0.0.1");
            store::migrate_notes(&pool).await?;
            store::backfill_board(&pool).await?;
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
            let card = store::read(&pool, workspace, &actor()?, &store::Scope::full()).await?;
            store::gate(&card, &subject)?;
            println!("explicit launch conditions accepted for this subject");
        }
        Some("read") => {
            let workspace = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("read WORKSPACE"))?;
            println!(
                "{}",
                store::read(&pool, workspace, &actor()?, &store::Scope::full()).await?
            );
        }
        Some("list") => println!("{}", store::list(&pool, &actor()?).await?),
        _ => bail!(
            "commands: migrate | backfill-board | serve [PORT HOST] | command WORKSPACE REQUEST_ID | read WORKSPACE | gate WORKSPACE | list"
        ),
    }
    Ok(())
}
fn actor() -> Result<String> {
    let actor = std::env::var("JOPPA_ACTOR").map_err(|_| {
        anyhow::anyhow!(
            "set JOPPA_ACTOR; server administration uses the identity assigned by its trusted launcher"
        )
    })?;
    joppa::commands::identifier(&actor)?;
    Ok(actor)
}
