use super::*;
use std::collections::BTreeMap;
pub(super) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    events: &[Event],
) -> Result<()> {
    let signals: BTreeMap<_, _> = state
        .objects
        .keys()
        .map(|key| (key, crate::objects::signal(state, key)))
        .collect();
    let summary = summaries(state);
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
    let first_view = json!({"projection_version":BOARD_PROJECTION_VERSION,"object_types":object_types,"custom_tree":custom_tree,"title":&state.title,"members":&state.members,
        "launch":{"ready":launch["ready"],"configured":configured,"blocker_count":blocker_count},"workspace_id":state.workspace_id.as_deref().unwrap_or(workspace),"counts":counts});
    super::objects::project(client, workspace, state, events).await?;
    client.execute("INSERT INTO joppa.board_cache(workspace,position,state,summary,signals,launch,first_view) VALUES($1,$2,$3::text::jsonb,$4::text::jsonb,$5::text::jsonb,$6::text::jsonb,$7::text::jsonb) ON CONFLICT (workspace) DO UPDATE SET position=EXCLUDED.position,state=EXCLUDED.state,summary=EXCLUDED.summary,signals=EXCLUDED.signals,launch=EXCLUDED.launch,first_view=EXCLUDED.first_view",
        &[&workspace,&(events.len() as i64),&serde_json::to_string(state)?,&summary.to_string(),&json!(signals).to_string(),&launch.to_string(),&first_view.to_string()]).await?;
    Ok(())
}

fn parsed(row: &tokio_postgres::Row, index: usize) -> Result<Value> {
    Ok(serde_json::from_str(row.get::<_, &str>(index))?)
}

pub async fn board_read(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    level: Option<&str>,
    id: Option<&str>,
    revision: Option<u32>,
) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    match level {
        None => {
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
            for label in conn.query("SELECT actor,split_part(email,'@',1) FROM joppa.google_identities WHERE actor=ANY($1)",&[&humans]).await? {
                let actor:String=label.get(0); labels[&actor]=json!(label.get::<_,String>(1));
            }
            Ok(
                json!({"workspace":first["workspace_id"],"position":row.get::<_,i64>(0),"lazy":true,
                "state":{"title":first["title"],"members":members},
                "member_labels":labels,"launch":first["launch"],
                "object_types":first["object_types"],"counts":first["counts"],"custom_tree":first["custom_tree"]}),
            )
        }
        Some("process") => {
            let key = id.context("process object address required")?;
            ensure!(revision.is_none(), "read document history through objects");
            let row = conn.query_opt(
                "WITH RECURSIVE chain AS (SELECT o.address,o.parent,o.process,ARRAY[o.address] AS seen FROM joppa.objects o WHERE o.workspace=$1 AND o.address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2) UNION ALL SELECT o.address,o.parent,o.process,c.seen||o.address FROM chain c JOIN joppa.objects o ON o.workspace=$1 AND o.address=c.parent WHERE NOT o.address=ANY(c.seen)) SELECT address,process->>'req' FROM chain WHERE process->>'req' IS NOT NULL ORDER BY cardinality(seen) ASC LIMIT 1",
                &[&workspace,&key],
            ).await?.context("no verification context")?;
            let context: String = row.get(0);
            let req: String = row.get(1);
            let row = conn.query_one(
                "SELECT (state->'requirements'->$2->'revisions'->-1)::text,(summary->$2)::text FROM joppa.board_cache WHERE workspace=$1",
                &[&workspace,&req],
            ).await?;
            let current = parsed(&row, 0)?;
            ensure!(!current.is_null(), "no verification record");
            let mut criteria = Vec::new();
            for row in conn.query(
                "SELECT o.address,COALESCE(o.process->>'criterion',o.local_id),o.title,o.signal::text FROM joppa.objects o JOIN joppa.board_cache b ON b.workspace=o.workspace WHERE o.workspace=$1 AND o.present AND o.process->>'req'=$2 AND b.first_view->'object_types'->o.kind->>'role'='ac' ORDER BY o.address",
                &[&workspace,&req],
            ).await? {
                criteria.push(json!({"address":row.get::<_,String>(0),"id":row.get::<_,String>(1),"title":row.get::<_,String>(2),"signal":parsed(&row,3)?}));
            }
            let number = current["number"]
                .as_u64()
                .context("verification record has no revision")?
                .to_string();
            let mut result_dates = json!({});
            for event in conn.query("SELECT command->>'run',command->'evidence'->>'check',recorded_at::text FROM joppa.events WHERE workspace=$1 AND command->>'req'=$2 AND command->>'revision'=$3 AND command->>'op'='result' ORDER BY position", &[&workspace,&req,&number]).await? {
                let run: String = event.get(0);
                let check: String = event.get(1);
                result_dates[&run][&check] = json!(event.get::<_,String>(2));
            }
            Ok(
                json!({"result_dates":result_dates,"address":context,"req":req,"revision":current["number"],"confirmed":current["confirmed"],"verification_scope":current["verification_scope"],"criteria":criteria,"runs":current["runs"],"issues":current["issues"],"acceptance":current["acceptance"],"summary":parsed(&row,1)?}),
            )
        }
        Some(other) => anyhow::bail!("unknown board level {other}"),
    }
}
