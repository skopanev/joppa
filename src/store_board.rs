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
    let selected = super::state_rows::affected(state, previous)?;
    let at = (events, position);
    project_selected(client, workspace, state, at, previous, selected).await
}

/// `project_current` with the objects that the write reaches (`state_rows::affected`), which
/// the caller has computed already.
pub(super) async fn project_selected(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    (events, position): (&[Event], u64),
    previous: Option<&Workspace>,
    selected: Option<std::collections::BTreeSet<String>>,
) -> Result<()> {
    super::state_rows::project(client, workspace, state, previous).await?;
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
    let object_types = crate::objects::schema(state).types;
    // The test type comes from migrate_test_nodes in every workspace; it does not make a tree custom.
    let mut compared = object_types.clone();
    compared.retain(|_, t| t.role != crate::test_nodes::ROLE);
    let mut defaults = crate::objects::defaults();
    defaults.retain(|_, t| t.role != crate::test_nodes::ROLE);
    let custom_tree = compared.len() != defaults.len()
        || compared.iter().any(|(kind, t)| {
            defaults.get(kind).is_none_or(|default| {
                (t.role.as_str(), &t.parents, t.root, &t.references)
                    != (
                        default.role.as_str(),
                        &default.parents,
                        default.root,
                        &default.references,
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
    let dimensions = crate::filter_dimensions::dimensions(state);
    let filters =
        crate::filter_fields::values(state, current.filters.as_deref().unwrap_or_default());
    // A typed enum field and a dimension offer each of their values.
    let filters =
        crate::filter_dimensions::with_values(filters, &crate::field_types::enum_values(state));
    let filters = crate::filter_dimensions::with_values(filters, &dimensions);
    let first_view = json!({"filters":filters,"dimensions":crate::filter_dimensions::listed(&dimensions),"computed_fields":crate::computed_fields::rules(&crate::objects::schema(state).types).keys().collect::<Vec<_>>(),"projection_version":BOARD_PROJECTION_VERSION,"readiness_metadata_version":READINESS_METADATA_VERSION,"object_types":object_types,"statuses":statuses,"custom_tree":custom_tree,"title":&state.title,"hidden":state.hidden,"assignees":crate::filter_fields::assignees(state),"members":&state.members,
        "workspace_id":state.workspace_id.as_deref().unwrap_or(workspace),"counts":counts});
    super::objects::project(client, workspace, state, events, selected.as_ref()).await?;
    client.execute("INSERT INTO joppa.board_cache(workspace,position,state,summary,signals,first_view) VALUES($1,$2,$3::text::jsonb,$4::text::jsonb,$5::text::jsonb,$6::text::jsonb) ON CONFLICT (workspace) DO UPDATE SET position=EXCLUDED.position,state=EXCLUDED.state,summary=EXCLUDED.summary,signals=EXCLUDED.signals,first_view=EXCLUDED.first_view || CASE WHEN board_cache.first_view ? 'legacy_receipts_version' THEN jsonb_build_object('legacy_receipts_version',board_cache.first_view->'legacy_receipts_version') ELSE '{}'::jsonb END || CASE WHEN board_cache.first_view ? 'status_projection' THEN jsonb_build_object('status_projection',board_cache.first_view->'status_projection') ELSE '{}'::jsonb END",
        &[&workspace,&(position as i64),&super::state_rows::metadata(state)?,&"{}",&"{}",&first_view.to_string()]).await?;
    // A write recomputes only the nodes that it reaches; a schema change rebuilds all.
    let reports: Vec<i64> = events
        .iter()
        .filter_map(|e| match &e.command {
            Command::RecordReport(op) => Some(op.report),
            _ => None,
        })
        .collect();
    let before = position - events.len() as u64;
    let updated = match (previous, selected) {
        (Some(previous), Some(dirty)) => {
            super::objects::update_status_projection(
                client,
                workspace,
                state,
                previous,
                (before, position),
                &reports,
                dirty,
            )
            .await?
        }
        _ => false,
    };
    if !updated {
        super::objects::rebuild_status_projection(client, workspace, state, position).await?;
    }
    Ok(())
}

/// The status list with the filter label of each tone.
fn labelled(statuses: &Value) -> Value {
    let mut statuses = statuses.clone();
    for s in statuses.as_array_mut().into_iter().flatten() {
        let label = crate::status_colors::label(s["tone"].as_str().unwrap_or_default());
        s["label"] = json!(label);
    }
    statuses
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
                "member_labels":labels,
                "object_types":first["object_types"],"statuses":labelled(&first["statuses"]),"archived_tone":crate::status_colors::ARCHIVED_TONE,"counts":first["counts"],"custom_tree":first["custom_tree"],"filters":first["filters"],"dimensions":first["dimensions"],"assignees":first["assignees"]}),
    )
}
