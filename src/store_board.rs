use super::*;
use std::collections::BTreeMap;
pub(super) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    events: &[Event],
) -> Result<()> {
    project_current(
        client,
        workspace,
        state,
        events,
        events.last().map_or(0, |e| e.position),
        None,
    )
    .await
}

pub(super) async fn project_current(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    events: &[Event],
    position: u64,
    previous: Option<&Workspace>,
) -> Result<()> {
    super::state_rows::project(client, workspace, state, previous).await?;
    let selected = super::state_rows::affected(state, previous)?;
    let summary = super::state_rows::changed_summaries(state, selected.as_ref());
    if let Some(summaries) = summary.as_object() {
        let rows: Vec<Value> = summaries
            .iter()
            .map(|(key, value)| json!({"key":key,"value":value}))
            .collect();
        if !rows.is_empty() {
            client.execute("INSERT INTO joppa.state_entries(workspace,section,key,value) SELECT $1,'summaries',x.key,x.value FROM jsonb_to_recordset($2::text::jsonb) x(key text,value jsonb) ON CONFLICT(workspace,section,key) DO UPDATE SET value=EXCLUDED.value WHERE state_entries.value IS DISTINCT FROM EXCLUDED.value", &[&workspace,&json!(rows).to_string()]).await?;
        }
    }
    let launch = crate::objects::launch_summary(state);
    let configured = &launch["configured"];
    let blocker_count = &launch["blocker_count"];
    let object_types = crate::objects::schema(state).types;
    let defaults = crate::objects::defaults();
    let custom_tree = object_types.len() != defaults.len()
        || object_types.iter().any(|(kind, t)| {
            defaults.get(kind).is_none_or(|default| {
                (
                    t.role.as_str(),
                    &t.parents,
                    t.root,
                    &t.parent_gate,
                    &t.references,
                    t.parent_decision,
                ) != (
                    default.role.as_str(),
                    &default.parents,
                    default.root,
                    &default.parent_gate,
                    &default.references,
                    default.parent_decision,
                )
            })
        });
    let mut counts = BTreeMap::<&str, usize>::new();
    for o in state.objects.values().filter(|o| o.current().present) {
        *counts.entry(&o.kind).or_default() += 1;
    }
    let current = crate::objects::schema(state);
    let statuses =
        crate::status_colors::effective(current.statuses.as_ref(), current.status_colors.as_ref());
    // The board fields of the result shape: the board filters by one value of one of them.
    let result_filters: Vec<Value> = crate::test_rules::rules(state)
        .map(|rules| {
            crate::result_values::board_fields(&rules, state)
                .into_iter()
                .map(|f| {
                    let title = rules
                        .fields
                        .iter()
                        .find(|r| r.name == f.name)
                        .and_then(|r| r.title.clone())
                        .unwrap_or_else(|| f.name.clone());
                    json!({"field":f.name,"title":title,"values":f.values})
                })
                .collect()
        })
        .unwrap_or_default();
    let first_view = json!({"projection_version":BOARD_PROJECTION_VERSION,"readiness_metadata_version":READINESS_METADATA_VERSION,"object_types":object_types,"statuses":statuses,"result_filters":result_filters,"custom_tree":custom_tree,"title":&state.title,"members":&state.members,
        "launch":{"ready":launch["ready"],"configured":configured,"blocker_count":blocker_count},"workspace_id":state.workspace_id.as_deref().unwrap_or(workspace),"counts":counts});
    super::objects::project(client, workspace, state, events, selected.as_ref()).await?;
    super::queue_rows::project(client, workspace, state, selected.as_ref()).await?;
    client.execute("INSERT INTO joppa.board_cache(workspace,position,state,summary,signals,launch,first_view) VALUES($1,$2,$3::text::jsonb,$4::text::jsonb,$5::text::jsonb,$6::text::jsonb,$7::text::jsonb) ON CONFLICT (workspace) DO UPDATE SET position=EXCLUDED.position,state=EXCLUDED.state,summary=EXCLUDED.summary,signals=EXCLUDED.signals,launch=EXCLUDED.launch,first_view=EXCLUDED.first_view || CASE WHEN board_cache.first_view ? 'legacy_receipts_version' THEN jsonb_build_object('legacy_receipts_version',board_cache.first_view->'legacy_receipts_version') ELSE '{}'::jsonb END",
        &[&workspace,&(position as i64),&super::state_rows::metadata(state)?,&"{}",&"{}",&launch.to_string(),&first_view.to_string()]).await?;
    super::objects::rebuild_status_projection(client, workspace, state, position).await?;
    Ok(())
}

fn parsed(row: &tokio_postgres::Row, index: usize) -> Result<Value> {
    Ok(serde_json::from_str(row.get::<_, &str>(index))?)
}

pub async fn board_read(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let row = conn
        .query_one(
            "SELECT position,first_view::text FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    let first = parsed(&row, 1)?;
    let members = &first["members"];
    let humans: Vec<String> = members
        .as_object()
        .into_iter()
        .flat_map(|o| o.iter())
        .filter(|(_, role)| role.as_str() == Some("human"))
        .map(|(id, _)| id.clone())
        .collect();
    let mut labels = json!({});
    for label in conn.query("SELECT actor,COALESCE(NULLIF(btrim(display_name),''),split_part(email,'@',1)) FROM joppa.google_identities WHERE actor=ANY($1)",&[&humans]).await? {
                let actor:String=label.get(0); labels[&actor]=json!(label.get::<_,String>(1));
            }
    Ok(
        json!({"workspace":first["workspace_id"],"position":row.get::<_,i64>(0),"lazy":true,
                "state":{"title":first["title"],"members":members},
                "member_labels":labels,"launch":first["launch"],
                "object_types":first["object_types"],"statuses":first["statuses"],"result_filters":first["result_filters"],"counts":first["counts"],"custom_tree":first["custom_tree"]}),
    )
}
