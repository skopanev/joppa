//! The alias of an actor: the name of the agent that writes under the account of a person,
//! such as p1 or runner. It changes no access: the actor of the session keeps every right.
//! The journal stores it with each event, and each revision, comment, file and readiness mark
//! of the event keeps it. An MCP call names it with the argument `alias`, or the session with
//! the header X-Joppa-Alias; the argument wins.
use anyhow::{Result, ensure};
use serde_json::Value;

/// The HTTP header that names the alias of every call of a session.
pub const HEADER: &str = "x-joppa-alias";

/// The largest number of characters of an alias.
pub const MAX: usize = 40;

pub const REFUSAL: &str = "alias has 1 to 40 letters, digits, \"-\", \"_\" or \".\"";

tokio::task_local! {
    static ALIAS: Option<String>;
}

/// Refuse an alias with another character, or with too few or too many.
pub fn validate(alias: &str) -> Result<()> {
    let count = alias.chars().count();
    ensure!(
        (1..=MAX).contains(&count)
            && alias
                .chars()
                .all(|c| c.is_alphanumeric() || "-_.".contains(c)),
        REFUSAL
    );
    Ok(())
}

/// The alias of one call: the argument `alias`, else the session header. The argument leaves
/// `args`, so a tool that refuses unknown fields reads the call without it.
pub fn take(args: &mut Value, header: Option<&str>) -> Result<Option<String>> {
    let argument = match args.as_object_mut().and_then(|a| a.remove("alias")) {
        None | Some(Value::Null) => None,
        Some(Value::String(alias)) => Some(alias),
        Some(_) => anyhow::bail!(REFUSAL),
    };
    let alias = argument.or_else(|| header.map(str::to_owned));
    if let Some(alias) = &alias {
        validate(alias)?;
    }
    Ok(alias)
}

/// Run `work` with `alias` as the alias of each write inside it.
pub async fn scope<F: std::future::Future>(alias: Option<String>, work: F) -> F::Output {
    ALIAS.scope(alias, work).await
}

/// Give `item` the key alias only when the write has one, so a read without an alias keeps
/// its earlier shape.
pub fn put(item: &mut Value, alias: Option<&str>) {
    if let Some(alias) = alias {
        item["alias"] = Value::from(alias);
    }
}

/// The alias of the current call, or None outside a call or without an alias.
pub fn current() -> Option<String> {
    ALIAS.try_with(Clone::clone).ok().flatten()
}
