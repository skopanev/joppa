//! The workspaces that name a workspace, and the removal of those references when the
//! owner deletes it. See `crate::workspace_references` for the state change.
use super::*;
use crate::workspace_references::{ACTOR, sources};

/// The live workspaces whose remote links or child workspace bindings name `workspace`.
async fn referencing(client: &impl GenericClient, workspace: &str) -> Result<Vec<String>> {
    Ok(client
        .query(
            "SELECT e.workspace FROM joppa.state_entries e CROSS JOIN LATERAL jsonb_array_elements(e.value->'links') l WHERE e.section='remote_links' AND l->>'workspace'=$1 AND e.workspace<>$1
             UNION SELECT o.workspace FROM joppa.objects o WHERE o.present AND o.document->'data'->>'workspace'=$1 AND o.workspace<>$1
             ORDER BY 1",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// Each workspace that names `workspace`, with its title and the objects that name it.
pub(super) async fn list(client: &impl GenericClient, workspace: &str) -> Result<Vec<Value>> {
    let mut out = vec![];
    for id in referencing(client, workspace).await? {
        let (state, _) = state_rows::current(client, &id).await?;
        if state.admin.is_empty() || state.deleted_at.is_some() {
            continue;
        }
        let objects = sources(&state, workspace);
        if !objects.is_empty() {
            out.push(json!({"id": id, "title": state.title, "objects": objects}));
        }
    }
    Ok(out)
}

/// The owner reads the workspaces that will lose their references to this workspace.
pub async fn workspace_references(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let (state, _) = state_rows::current(&**conn, &workspace).await?;
    ensure!(actor == state.admin, "workspace owner required");
    Ok(json!({"id": workspace, "workspaces": list(&**conn, &workspace).await?}))
}

/// Append one internal event to each workspace that names `workspace`, under its lock. The
/// event removes the references. Return the workspaces and the objects that changed.
pub(super) async fn remove(
    client: &impl GenericClient,
    workspace: &str,
    title: &str,
) -> Result<Vec<Value>> {
    let changed = list(client, workspace).await?;
    let reason = format!("The owner deleted the workspace {title}. Its links are removed.");
    for item in &changed {
        let id = item["id"].as_str().context("workspace ID")?;
        client
            .execute(
                "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
                &[&id],
            )
            .await?;
        let (mut state, base) = state_rows::current(client, id).await?;
        let previous = state.clone();
        let command = Command::RemoveWorkspaceReferences {
            workspace: workspace.to_owned(),
            reason: reason.clone(),
        };
        let position = base + 1;
        let prefix = state
            .reference_prefix
            .clone()
            .unwrap_or_else(|| id.to_owned());
        let at: String = client.query_one("WITH context AS MATERIALIZED (SELECT set_config('joppa.reference_prefix',$6,true)) INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,$2,$3,$4,$5::text::jsonb FROM context RETURNING recorded_at::text", &[&id,&(position as i64),&ACTOR,&uuid::Uuid::new_v4().to_string(),&serde_json::to_string(&command)?,&prefix]).await?.get(0);
        commands::apply(&mut state, ACTOR, &command, position, &at)?;
        let events = [Event {
            position,
            actor: ACTOR.into(),
            actor_type: crate::inbox::actor_type(&state, ACTOR).into(),
            recorded_at: at,
            command,
        }];
        let selected = state_rows::affected(&state, Some(&previous))?;
        search::project(client, id, &state, position, selected.as_ref()).await?;
        board::project_current(client, id, &state, &events, position, Some(&previous)).await?;
    }
    Ok(changed)
}

/// Remove the references to workspaces that no longer exist, for deletions before the
/// purge. A reference to an unknown name stays: it may be an alias.
pub(super) async fn remove_dangling(client: &impl GenericClient) -> Result<()> {
    let gone: Vec<String> = client
        .query(
            "SELECT DISTINCT l->>'workspace' FROM joppa.state_entries e CROSS JOIN LATERAL jsonb_array_elements(e.value->'links') l WHERE e.section='remote_links'
             UNION SELECT DISTINCT o.document->'data'->>'workspace' FROM joppa.objects o WHERE o.present AND o.document->'data'->>'workspace' IS NOT NULL",
            &[],
        )
        .await?
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect();
    for workspace in gone {
        let known = client
            .query_opt(
                "SELECT 1 FROM joppa.workspaces WHERE id=$1 UNION SELECT 1 FROM joppa.workspace_aliases WHERE name=$1",
                &[&workspace],
            )
            .await?
            .is_some();
        if !known {
            remove(client, &workspace, &workspace).await?;
        }
    }
    Ok(())
}
