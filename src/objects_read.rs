//! The object read: one object by its address, or one page of a list.
use super::*;

pub async fn read_objects(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &ObjectScope<'_>,
) -> Result<Value> {
    let (mut conn, workspace) = authorized(pool, workspace, actor).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let row=conn.query_one("SELECT position,COALESCE(state->>'workspace_id',workspace),COALESCE(state->>'reference_prefix',workspace),(first_view->'object_types')::text FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?;
    let position: i64 = row.get(0);
    let public: String = row.get(1);
    let prefix: String = row.get(2);
    let selected: std::collections::BTreeMap<String, objects::TreeType> =
        serde_json::from_str(row.get(3))?;
    if let Some(key) = scope.address {
        let id = workspace.clone();
        let mut object = read_one(&conn, workspace, public, position, prefix, scope, key).await?;
        // Links and child workspaces across workspaces: read now, with the access of the reader.
        if scope.revision.is_none() && !scope.history {
            crate::store::remote_status::decorate(&*conn, &id, actor, &mut object).await?;
        }
        return Ok(object);
    }
    super::object_list::read_list(&conn, workspace, public, position, prefix, selected, scope).await
}

#[path = "objects_read_one.rs"]
mod one;
use one::read_one;
