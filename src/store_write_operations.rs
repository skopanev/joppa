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
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    // A claim, a completion and an acceptance read the workspaces of blocking remote links,
    // so they lock them with their own workspace in sorted ID order. A record of results
    // there waits for them, or comes first.
    let claims = receipts::locks_remote(operations);
    let remote_locked = if claims {
        super::remote::blocking_closure(&*tx, &workspace).await?
    } else {
        Default::default()
    };
    super::remote::lock_sorted(&*tx, &workspace, &remote_locked).await?;
    // A new workspace has no access-rule entry yet. Only the trusted CLI path
    // may create it; public HTTP/MCP writes always go through access::check.
    let cli_create = allow_workspace_create && operations[0].command["op"] == "workspace";
    if !cli_create {
        access::check(&*tx, &workspace, actor, false).await?;
    }
    ensure!(actor != "joppa-access", "internal enrollment operation");
    if let Some(reply) = receipts::replayed(
        &*tx, &workspace, actor, request_id, operations, batch, &input,
    )
    .await?
    {
        return Ok(reply);
    }
    let (mut state, base_position) = state_rows::current(&*tx, &workspace).await?;
    let mut previous = state.clone();
    let mut events = Vec::new();
    receipts::migrate_claims(&*tx, &workspace, &mut state, &mut events, base_position).await?;
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
            generate(&mut value)?;
            fields(&value)?;
            // The journal stores the immutable ID of another workspace, never a name.
            super::remote::canonical_ids(&*tx, &mut value).await?;
            if value["op"] == "claim_next_task" {
                state.claim_time_ms = Some(tx.query_one("SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint", &[]).await?.get(0));
            }
            resolve_task(&mut value, &state)?;
            // Work waits for its unsatisfied ordering dependencies; the wait is never stored.
            let waits = if matches!(value["op"].as_str(), Some("claim_next_task" | "claim_task" | "claim_planning")) {
                super::waits::live(&*tx, &workspace, actor, &state, &remote_locked).await?
            } else {
                BTreeMap::new()
            };
            resolve_claim_next(&mut value, &state, actor, &waits)?;
            state_rows::hydrate_command(&*tx, &workspace, &mut state, &mut previous, &value).await?;
            state_rows::hydrate_revision(&*tx, &workspace, &mut state, &value).await?;
            let command: Command = serde_json::from_value(value)?;
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
            if let Command::SaveObject(crate::commands::SaveObjectOp { kind, question_checked, .. }) = &command {
                let schema = crate::objects::schema(&state);
                if let Some(question) = schema.types.get(kind).and_then(|t| t.question.as_ref())
                    && question_checked.as_ref() != Some(question) {
                    return Err(crate::objects::RequiredField(format!("Check that the title and body answer: {question}. Rewrite them if they do not. Then resend with question_checked")).into());
                }
            }
            if let Command::Revise(crate::commands::ReviseOp { req, consumers, capability, acs, .. }) = &command {
                ensure!(capability.is_some() || req.contains(':'), "legacy revise requires a Capability; use save_object for a custom tree");
                if state.schema_migrated {
                    let mut data = state.objects.get(&crate::objects::requirement_address(req)).map(|o| o.current().data.clone()).unwrap_or_else(|| json!({}));
                    data["consumers"] = json!(consumers);
                    let req_address = crate::objects::requirement_address(req);
                    let kind = req_address.split_once(':').unwrap().0;
                    crate::objects::reference_targets(&state, kind, &data)?;
                    crate::consumer_migration::check_revise(&state, consumers, acs)?;
                } else { ensure!(!consumers.is_empty(), "REQ requires at least one consumer"); }
            }
            if let Command::AcceptCommit { target, commit: Some(commit), .. } = &command {
                super::reports::check_acceptance(&*tx, &workspace, &state, target, commit).await?;
                // An acceptance needs every blocking remote target of the node, its ancestors
                // and the nodes below it ready now.
                if let Some(target) = super::waits::remote_gate(&*tx, actor, &state, target, true, &remote_locked).await? {
                    anyhow::bail!("{}", crate::dependencies::reason(&target));
                }
            }
            if let Command::CompleteTask(crate::commands::CompleteTaskOp { req, id, .. }) = &command {
                let task = crate::objects::resolve_address(&state, &crate::tasks::address(req, id)).to_owned();
                if let Some(target) = super::waits::remote_gate(&*tx, actor, &state, &task, false, &remote_locked).await? {
                    anyhow::bail!("{}", crate::dependencies::reason(&target));
                }
            }
            if let Command::ClaimPlanning { ac, revision } = &command {
                let target = crate::planning::validate_claim(&state, ac, *revision)?;
                let status = super::reports::planning_criterion_status(&*tx, &workspace, &state, &target).await?;
                ensure!(status["status"] != "green", "criterion is already verified");
                if let Some(dependency) = waits.get(&target) {
                    anyhow::bail!("{}", crate::dependencies::reason(dependency));
                }
            }
            // Only server code enrolls an agent or a runner; HTTP and MCP writes refuse it.
            ensure!(
                allow_workspace_create || !matches!(command, Command::EnrollService { .. }),
                "internal operation"
            );
            ensure!(!matches!(command, Command::EnrollMember { .. } | Command::RemoveWorkspaceReferences { .. } | Command::RecordReport(crate::commands::RecordReportOp { .. }) | Command::MigrateNotes {} | Command::MigrateTasks {} | Command::MigrateSchema {} | Command::MigrateDocuments {} | Command::MigrateTaskClaims {} | Command::MigratePersistentClaims {} | Command::MigrateConsumers {} | Command::MigrateConsumerDrops {} | Command::MigrateTestRules {} | Command::MigrateNoteParents {} | Command::MigrateBody {}), "internal operation");
            ensure!(
                allow_workspace_create || !matches!(command, Command::Workspace { .. }),
                "workspace creation is available only through the server CLI"
            );
            let mut position = base_position + events.len() as u64;
            // A save that changes nothing keeps the current revision and adds no journal event.
            if !commands::unchanged_save(&state, actor, &command, position + 1, "")? {
                position += 1;
                if position == 1 {
                    tx.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&workspace]).await?;
                    super::start_replay_epochs(&*tx, &workspace).await?;
                    reserve_name(&*tx, &workspace, &workspace).await?;
                }
                if let Command::RenameWorkspace(crate::commands::RenameWorkspaceOp { workspace_id: Some(name), .. }) = &command {
                    reserve_name(&*tx, name, &workspace).await?;
                }
                let event_request = if batch { format!("batch-{batch_id}-{index}") } else { request_id.to_owned() };
                let payload = serde_json::to_string(&command)?;
                let recorded_at:String=tx.query_one("WITH context AS MATERIALIZED (SELECT set_config('joppa.reference_prefix',$6,true)) INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,$2,$3,$4,$5::text::jsonb FROM context RETURNING recorded_at::text", &[&workspace,&(position as i64),&actor,&event_request,&payload,&state.reference_prefix.as_deref().unwrap_or(&workspace)]).await?.get(0);
                commands::apply(&mut state, actor, &command, position, &recorded_at)?;
                // The Task pin, block and hold refusals come first; the refused write rolls back.
                if let Command::ClaimTask { req, id, .. } = &command
                    && let Some(dependency) = waits.get(crate::objects::resolve_address(&state, &crate::tasks::address(req, id)))
                {
                    anyhow::bail!("{}", crate::dependencies::reason(dependency));
                }
                crate::objects::validate_authoring_questions(&state, &command, position)?;
                crate::objects::validate_tree(&state)?;
                events.push(Event{position,actor:actor.to_owned(),actor_type:crate::inbox::actor_type(&state, actor).into(),recorded_at,command:command.clone()});
            }
            let mut resolved=serde_json::to_value(&command)?;
            let saved_address = match &command {
                Command::SaveObject(crate::commands::SaveObjectOp { id, kind, .. }) => Some(format!("{kind}:{id}")),
                Command::SetNoteStatus(crate::commands::SetNoteStatusOp { target, .. }) | Command::DecideNote(crate::commands::DecideNoteOp { target, .. }) => Some(crate::objects::resolve_address(&state, target).to_owned()),
                _ => None,
            };
            // A later command of the batch names a saved object as {"$ref":"label/address"}.
            if let (Command::SaveObject(crate::commands::SaveObjectOp { .. }), Some(address)) = (&command, &saved_address) { resolved["address"] = json!(address); }
            let saved=if let Some(address)=saved_address {
                let revision=state.objects[&address].current().revision;
                resolved["address"]=json!(address);resolved["saved_revision"]=json!(revision);
                let mut saved = json!({"address":address,"revision":revision});
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
    super::board::project_current(
        &*tx,
        &workspace,
        &state,
        &events,
        base_position + events.len() as u64,
        Some(&previous),
    )
    .await?;
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
    tx.commit().await?;
    if purged {
        super::remove_folder(&workspace).await;
    }
    super::search_indexer::wake();
    Ok(result)
}
