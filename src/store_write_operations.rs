//! Part of `store_write`: the write of one request, moved from store_write.rs unchanged.
use super::*;

pub(crate) async fn write(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    operations: &[BatchOperation],
    batch: bool,
    allow_workspace_create: bool,
) -> Result<Value> {
    commands::identifier(workspace)?;
    commands::identifier(request_id)?;
    ensure!(
        !operations.is_empty() && operations.len() <= 100,
        "supply between 1 and 100 operations"
    );
    if !allow_workspace_create {
        admit(operations, batch)?;
    }
    let input = json!({"batch":batch, "operations":operations});
    let name = workspace;
    // The writers of one workspace wait here, so they hold at most 1 connection.
    let _turn = super::writer::turn(name).await;
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    super::remote::lock(&*tx, &workspace).await?;
    // A new workspace has no access-rule entry yet. Only the trusted CLI path
    // may create it; public HTTP/MCP writes always go through access::check.
    let cli_create = allow_workspace_create && operations[0].command["op"] == "workspace";
    if !cli_create {
        access::check(&*tx, &workspace, actor, false).await?;
    }
    ensure!(actor != "joppa-access", "internal enrollment operation");
    super::writer::resolved(name, &workspace);
    if let Some(reply) = receipts::replayed(
        &*tx, &workspace, actor, request_id, operations, batch, &input,
    )
    .await?
    {
        return Ok(reply);
    }
    let (mut state, base_position) = super::state_cache::take(&*tx, &workspace).await?;
    let mut previous = state.clone();
    let mut events = Vec::new();
    let mut refs = BTreeMap::new();
    let mut results = vec![];
    let batch_id = Uuid::new_v4();
    for (index, operation) in operations.iter().enumerate() {
        let apply = async {
            commands::reject_actor_field(&operation.command)?;
            if let Some(name) = &operation.reference {
                commands::identifier(name)?;
                ensure!(!refs.contains_key(name), "duplicate batch reference");
            }
            let mut value = resolve(&operation.command, &refs)?;
            let clean = crate::save_merge::take_clean(&mut value)?;
            generate_free(&*tx, &workspace, &state, &mut value).await?;
            fields(&value)?;
            // The journal stores the immutable ID of another workspace, never a name.
            super::remote::canonical_ids(&*tx, &mut value).await?;
            super::public_ids::canonical(&*tx, &workspace, &state, &mut value).await?;
            state_rows::hydrate_command(&*tx, &workspace, &mut state, &mut previous, &value).await?;
            state_rows::hydrate_revision(&*tx, &workspace, &mut state, &value).await?;
            crate::save_merge::merge(&state, &mut value, clean)?;
            let mut command: Command = serde_json::from_value(value)?;
            crate::omitted_links::keep(&state, &mut command);
            if let Command::SetRemoteLinks { target, links, .. } = &command {
                let source = crate::objects::resolve_address(&state, target).to_owned();
                super::remote::check_links(&*tx, &workspace, actor, &source, links).await?;
            }
            if let Command::SaveObject(crate::commands::SaveObjectOp { data, .. }) = &command
                && let Some(child) = data.get("workspace")
            {
                let child = child.as_str().context("workspace is a workspace ID")?;
                super::remote::check_child(&*tx, &workspace, actor, child).await?;
            }
            // Only server code enrolls an agent or a runner; HTTP and MCP writes refuse it.
            ensure!(
                allow_workspace_create || !matches!(command, Command::EnrollService { .. }),
                "internal operation"
            );
            ensure!(!matches!(command, Command::EnrollMember { .. } | Command::RemoveWorkspaceReferences { .. } | Command::RecordReport(crate::commands::RecordReportOp { .. }) | Command::MigrateNotes {} | Command::MigrateTasks {} | Command::MigrateSchema {} | Command::MigrateDocuments {} | Command::MigrateTaskClaims {} | Command::MigratePersistentClaims {} | Command::MigrateConsumers {} | Command::MigrateConsumerDrops {} | Command::MigrateTestRules {} | Command::MigrateNoteParents {} | Command::MigrateBody {} | Command::MigratePurpose {} | Command::MigrateTestNodes {} | Command::MigrateGenericNodes {} | Command::MigrateDecisionRequired {}), "internal operation");
            ensure!(
                allow_workspace_create || !matches!(command, Command::Workspace { .. }),
                "workspace creation is available only through the server CLI"
            );
            let mut position = base_position + events.len() as u64;
            let alias = crate::actor_alias::current();
            // A save that changes nothing keeps the current revision and adds no journal event.
            if !commands::unchanged_save(&state, actor, &command, position + 1, "")? {
                position += 1;
                if position == 1 {
                    tx.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&workspace]).await?;
                    super::start_replay_epochs(&*tx, &workspace).await?;
                    reserve_name(&*tx, &workspace, &workspace).await?;
                }
                if let Command::UpdateWorkspace(crate::commands::UpdateWorkspaceOp { workspace_id: Some(name), .. }) = &command {
                    reserve_name(&*tx, name, &workspace).await?;
                }
                let event_request = if batch { format!("batch-{batch_id}-{index}") } else { request_id.to_owned() };
                let payload = serde_json::to_string(&command)?;
                let recorded_at:String=tx.query_one("WITH context AS MATERIALIZED (SELECT set_config('joppa.reference_prefix',$6,true)) INSERT INTO joppa.events(workspace,position,actor,request_id,command,alias) SELECT $1,$2,$3,$4,$5::text::jsonb,$7 FROM context RETURNING recorded_at::text", &[&workspace,&(position as i64),&actor,&event_request,&payload,&state.reference_prefix.as_deref().unwrap_or(&workspace),&alias]).await?.get(0);
                state.alias = alias.clone();
                commands::apply_checked(&mut state, actor, &command, position, &recorded_at)?;
                crate::objects::validate_tree(&state)?;
                events.push(Event{position,actor:actor.to_owned(),recorded_at,command:command.clone()});
            }
            let mut resolved=serde_json::to_value(&command)?;
            let saved_address = match &command {
                Command::SaveObject(crate::commands::SaveObjectOp { id, kind, .. }) => Some(format!("{kind}:{id}")),
                _ => None,
            };
            let saved=if let Some(address)=saved_address {
                let current=state.objects[&address].current();let revision=current.revision;
                // A later command of the batch names a saved object as {"$ref":"label/address"}.
                resolved["address"]=json!(address);resolved["saved_revision"]=json!(revision);
                let mut saved = json!({"address":address,"revision":revision});
                // The current revision of the parent, for a child write that pins it.
                if let Some(parent)=current.parent.as_deref().and_then(|p| state.objects.get(crate::objects::resolve_address(&state,p))) { saved["parent_revision"]=json!(parent.current().revision); }
                if let Command::SaveObject(crate::commands::SaveObjectOp { kind, .. }) = &command
                    && let Some(question) = crate::objects::schema(&state).types[kind].question.as_ref() {
                    saved["question"] = json!(question);
                }
                if let Some(hint) = crate::ac_tests::draft_hint(&state, &address) {
                    saved["tests_draft"] = json!(true);
                    saved["hint"] = json!(hint);
                }
                Some(saved)
            }else{None};
            if let Some(name) = &operation.reference { refs.insert(name.clone(), resolved); }
            Ok::<_, anyhow::Error>(json!({"position":position,"command":command,"object":saved,"system_ids":item_ids(&*tx,&workspace,&state,&command).await?}))
        }.await;
        let apply = if batch {
            apply.with_context(|| format!("operation {}", index + 1))
        } else {
            apply
        }?;
        results.push(apply);
    }
    ensure_notes(&*tx, &workspace, &mut state, &mut events, base_position).await?;
    let selected = state_rows::affected(&state, Some(&previous))?;
    super::search::project(
        &*tx,
        &workspace,
        &state,
        base_position + events.len() as u64,
        selected.as_ref(),
    )
    .await?;
    let at = (events.as_slice(), base_position + events.len() as u64);
    let reached = selected.clone();
    super::board::project_selected(&*tx, &workspace, &state, at, Some(&previous), reached).await?;
    super::search_revisions::project(&*tx, &workspace, selected.as_ref()).await?;
    let mut result = if batch {
        json!({"results":results})
    } else {
        results.remove(0)
    };
    result["replayed"] = json!(false);
    result["summary"] = state_rows::changed_summaries(&state, selected.as_ref());
    tx.execute("INSERT INTO joppa.command_receipts(workspace,request_id,actor,input,result) VALUES($1,$2,$3,$4::text::jsonb,$5::text::jsonb)", &[&workspace,&request_id,&actor,&input.to_string(),&result.to_string()]).await?;
    if base_position == 0 {
        tx.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{legacy_receipts_version}',to_jsonb($2::text)) WHERE workspace=$1", &[&workspace,&LEGACY_RECEIPTS_VERSION]).await?;
    }
    // A removed member loses the email grant of its Google account in the same transaction: a
    // refused removal keeps the grant, and a grant never admits a removed member again.
    for event in &events {
        if let Command::RemoveMember { id, .. } = &event.command {
            tx.execute("DELETE FROM joppa.email_grants g USING joppa.google_identities i WHERE g.workspace=$1 AND i.actor=$2 AND g.email=lower(i.email)", &[&workspace, id]).await?;
        }
    }
    // A deleted workspace leaves the database with every row of it, this request included.
    let purged = events
        .iter()
        .any(|e| matches!(e.command, Command::DeleteWorkspace { .. }));
    if purged {
        // The workspaces that named it lose those references first, each with its own event.
        result["removed_references"] =
            json!(super::references::remove(&*tx, &workspace, &state.title).await?);
        tx.execute("SELECT joppa.purge_workspace($1)", &[&workspace])
            .await?;
    }
    // A deleted workspace keeps no state in memory.
    let kept = if purged {
        None
    } else {
        super::state_cache::settle(&*tx, &workspace, state).await?
    };
    tx.commit().await?;
    if let Some(kept) = kept {
        kept.keep();
    }
    if purged {
        super::remove_folder(&workspace).await;
    }
    super::search_indexer::wake();
    Ok(result)
}
