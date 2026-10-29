//! The object read: one object by its address, or one page of a list.
use super::*;

pub async fn read_objects(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &ObjectScope<'_>,
) -> Result<Value> {
    read(pool, workspace, actor, scope, true).await
}

/// The read of a writer that saves the document again: it keeps every stored field.
pub async fn read_stored_objects(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &ObjectScope<'_>,
) -> Result<Value> {
    read(pool, workspace, actor, scope, false).await
}

async fn read(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &ObjectScope<'_>,
    hide: bool,
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
    let kinds_for = |role: &str| {
        selected
            .iter()
            .filter(|(_, t)| t.role == role)
            .map(|(kind, _)| kind.clone())
            .collect::<Vec<_>>()
    };

    let task_kinds = kinds_for("task");
    let criterion_kinds = kinds_for("ac");
    let specification_kinds = kinds_for("requirement");
    ensure!(
        scope.work_filter.is_none_or(|filter| filter == "unstarted"),
        "unknown work filter"
    );
    let unstarted = scope.work_filter == Some("unstarted");
    if let Some(key) = scope.address {
        let id = workspace.clone();
        let mut object = read_one(&conn, workspace, public, position, prefix, scope, key).await?;
        if hide && object["historical"] == false {
            let t = object["kind"].as_str().and_then(|kind| selected.get(kind));
            crate::consumer_migration::hide_requirement_consumers(t, &mut object["document"]);
        }
        // Links and child workspaces across workspaces: read now, with the access of the reader.
        if scope.revision.is_none() && !scope.history {
            crate::store::remote_status::decorate(&*conn, &id, actor, &mut object).await?;
        }
        return Ok(object);
    }
    super::object_list::read_list(
        &conn,
        workspace,
        public,
        position,
        prefix,
        selected,
        task_kinds,
        criterion_kinds,
        specification_kinds,
        unstarted,
        scope,
    )
    .await
}

#[path = "objects_read_one.rs"]
mod one;
use one::read_one;
