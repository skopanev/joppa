//! The tree schema read.
use super::*;

pub async fn tree_schema(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    revision: Option<u32>,
) -> Result<Value> {
    let (conn, workspace) = authorized(pool, workspace, actor).await?;
    let requested = revision.map(i64::from);
    let row=conn.query_one("SELECT state->>'admin',COALESCE((state->'tree_schema'->-1->>'revision')::integer,0),COALESCE(state->>'workspace_id',workspace),position,(SELECT value::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' AND revision=COALESCE($2::bigint,(state->'tree_schema'->-1->>'revision')::bigint)) FROM joppa.board_cache WHERE workspace=$1", &[&workspace,&requested]).await?;
    let current: i32 = row.get(1);
    let n = revision.unwrap_or(current as u32);
    let selected = if n == 0 {
        objects::SchemaRevision {
            revision: 0,
            types: objects::legacy_defaults(),
            priorities: None,
            actor: row.get(0),
            at: String::new(),
            position: 0,
            reason: "Default workspace tree".into(),
        }
    } else {
        serde_json::from_str::<objects::SchemaRevision>(
            &row.get::<_, Option<String>>(4)
                .context("unknown schema revision")?,
        )?
    };
    let types =
        access::actor_types(&**conn, &workspace, std::slice::from_ref(&selected.actor)).await?;
    let mut schema = json!(selected);
    schema["actor_type"] = json!(types[&selected.actor]);
    Ok(
        json!({"workspace":row.get::<_,String>(2),"position":row.get::<_,i64>(3),"current_revision":current,"schema":schema}),
    )
}
