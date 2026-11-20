//! Rows read as a stream: each row becomes its value at once, and no list of rows of a large
//! workspace stays in memory.
use anyhow::Result;
use tokio_postgres::{GenericClient, Row, types::ToSql};

/// Give each row of `sql` to `each`, one row at a time.
pub(crate) async fn each(
    client: &impl GenericClient,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
    mut each: impl FnMut(Row) -> Result<()>,
) -> Result<()> {
    let rows = client.query_raw(sql, params.iter().copied()).await?;
    futures_util::pin_mut!(rows);
    while let Some(row) = futures_util::TryStreamExt::try_next(&mut rows).await? {
        each(row)?;
    }
    Ok(())
}
