//! Rows read as a stream: each row becomes its value at once, and no list of rows of a large
//! workspace stays in memory.
use anyhow::Result;
use tokio_postgres::{GenericClient, Row, types::ToSql};

/// The rows of one statement of a bulk write: a large workspace writes its rows in parts.
pub(crate) const PART: usize = 500;

/// Give the freed memory of the heap back to the system after a large operation. glibc keeps
/// the freed pages of its arenas otherwise, and the server grows with each large rebuild.
pub(crate) fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: malloc_trim only returns free pages of the glibc heap to the system.
        unsafe {
            malloc_trim(0);
        }
    }
}

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
