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
            status_colors: None,
            statuses: None,
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
    // Readers see the status list that applies, also when the schema keeps the default list.
    schema["statuses"] = json!(crate::status_colors::effective(
        selected.statuses.as_ref(),
        selected.status_colors.as_ref()
    ));
    // The tree page shows the values that apply: priority levels, link relations and the
    // status rule of each type.
    schema["priority_levels"] = json!(
        selected
            .priorities
            .clone()
            .unwrap_or_else(|| { ["p0", "p1", "p2"].map(String::from).to_vec() })
    );
    schema["link_relations"] = json!(objects::LINK_RELATIONS);
    schema["status_rules"] = json!(
        selected
            .types
            .iter()
            .map(|(kind, t)| (kind.clone(), objects::status_rule(t)))
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    Ok(
        json!({"workspace":row.get::<_,String>(2),"workspace_key":workspace,"position":row.get::<_,i64>(3),"current_revision":current,"schema":schema}),
    )
}
