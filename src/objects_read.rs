//! The object read: one object by its address, or one page of a list.
use super::*;

pub async fn read_objects(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &ObjectScope<'_>,
) -> Result<Value> {
    read(pool, workspace, actor, scope, None).await
}

/// The members of relation `key` (src/relation_members.rs): at most `limit` present nodes
/// after the address `cursor`, in address order. `data` also gives the data and the
/// relations of each item. Each of `keys` must be a relation of the registry, as `key`.
/// next_cursor is the bare address of the last item.
pub async fn relation_members(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    key: &str,
    (limit, cursor, data, keys): (usize, Option<&str>, bool, &[String]),
) -> Result<Value> {
    ensure!((1..=25).contains(&limit), "limit must be 1 to 25");
    let scope = ObjectScope {
        limit,
        cursor,
        data,
        ..ObjectScope::default()
    };
    let mut reply = read(pool, workspace, actor, &scope, Some((key, keys))).await?;
    if let Some((_, address)) = reply["next_cursor"]
        .as_str()
        .and_then(|c| c.split_once(':'))
        .filter(|(p, _)| p.parse::<i64>().is_ok())
    {
        reply["next_cursor"] = json!(address);
    }
    Ok(reply)
}

async fn read(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &ObjectScope<'_>,
    member_read: Option<(&str, &[String])>,
) -> Result<Value> {
    let in_relation = member_read.map(|(key, _)| key);
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
        // The read-only transaction ends here, so no ROLLBACK runs later on drop.
        conn.commit().await?;
        return Ok(object);
    }
    if let Some((key, keys)) = member_read {
        let registry = registry(&conn, &workspace).await?;
        ensure!(
            std::iter::once(key)
                .chain(keys.iter().map(String::as_str))
                .all(|k| registry.contains_key(k)),
            "unknown link relation"
        );
    }
    let reply = super::object_list::read_list(
        &conn,
        workspace,
        public,
        position,
        prefix,
        selected,
        scope,
        in_relation,
    )
    .await?;
    conn.commit().await?;
    Ok(reply)
}

/// The relation keys of the current schema revision. A revision without a registry, and a
/// workspace without a schema, have the system relations.
async fn registry(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: &str,
) -> Result<std::collections::BTreeMap<String, Value>> {
    let row = conn.query_opt("SELECT (value->'relations')::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' AND revision=(SELECT (state->'tree_schema'->-1->>'revision')::bigint FROM joppa.board_cache WHERE workspace=$1)", &[&workspace]).await?;
    match row.and_then(|r| r.get::<_, Option<String>>(0)) {
        Some(text) => Ok(serde_json::from_str(&text)?),
        None => Ok(crate::relations::system()
            .into_keys()
            .map(|key| (key, Value::Null))
            .collect()),
    }
}

#[path = "objects_read_one.rs"]
mod one;
use one::read_one;
