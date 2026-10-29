//! A copied public ID resolves through search, as an agent does with joppa_search.
use anyhow::{Result, ensure};

/// The address `kind:id` of the object whose current or kept public ID is `id`.
pub async fn resolve(
    pool: &deadpool_postgres::Pool,
    actor: &str,
    workspace: &str,
    id: &str,
) -> Result<String> {
    let found = joppa::store::search::find(pool, actor, workspace, id, None, 1, None).await?;
    let first = &found["items"][0];
    ensure!(
        first["exact_id"] == true,
        "{id} is not an exact match: {found}"
    );
    Ok(format!(
        "{}:{}",
        first["kind"].as_str().unwrap_or_default(),
        first["id"].as_str().unwrap_or_default()
    ))
}
