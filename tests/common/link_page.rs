//! The incoming related Notes of one object, as one group read of the store.
use joppa::store;
use serde_json::Value;

/// The first 100 incoming `related` links from Notes to `address`, read by `agent`.
pub async fn incoming_notes(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    address: &str,
) -> anyhow::Result<Value> {
    let filter = (Some("incoming"), Some("related"), Some("note"));
    store::link_page(
        pool, ws, "agent", address, filter.0, filter.1, filter.2, 100, None,
    )
    .await
}
