//! Part of `objects_projection`: the writes of one projection column.
use super::*;

/// Store one value per object in `column`, and clear it on every other object of the
/// workspace. Only rows whose value changes are written. The rows go in parts of PART, and one
/// statement clears the objects outside the rows, so a part never clears another part.
pub(in crate::store) async fn store_column(
    client: &impl GenericClient,
    workspace: &str,
    column: &str,
    rows: &[Value],
) -> Result<()> {
    store_rows(client, workspace, column, rows).await?;
    let listed: Vec<&str> = rows.iter().filter_map(|r| r["address"].as_str()).collect();
    let sql = format!(
        "UPDATE joppa.objects SET {column}=NULL WHERE workspace=$1 AND {column} IS NOT NULL AND NOT (address=ANY($2))"
    );
    client.execute(&sql, &[&workspace, &listed]).await?;
    Ok(())
}

/// Store one value for each listed object in `column`. Every other row keeps its value.
pub(in crate::store) async fn store_rows(
    client: &impl GenericClient,
    workspace: &str,
    column: &str,
    rows: &[Value],
) -> Result<()> {
    let sql = format!(
        "UPDATE joppa.objects o SET {column}=x.value FROM jsonb_to_recordset($2::text::jsonb) x(address text,value jsonb) WHERE o.workspace=$1 AND o.address=x.address AND o.{column} IS DISTINCT FROM x.value"
    );
    for part in rows.chunks(crate::store::stream::PART) {
        client
            .execute(&sql, &[&workspace, &json!(part).to_string()])
            .await?;
    }
    Ok(())
}
