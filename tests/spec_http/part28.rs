// Part 28 of tests/spec_http.rs, included there, so the test names stay unchanged.

// signal-becomes-spec-ac2
// A member may archive an Inbox object, so the archive refusal uses a capability.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn restricted_operations_refuse_callers_that_help_does_not_name() -> Result<()> {
    let w = world().await?;
    let help = mcp_value(&w, "joppa_help", json!({})).await?;
    let revision = async |address: &str| -> Result<Value> {
        Ok(w.get(&format!("address={address}")).await?["current_revision"].clone())
    };
    let owner_only = "workspace owner required";
    let agent_or_owner = "workspace owner or agent required";
    let mut covered = vec![];
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["job"] = json!({"title":"Jobs","role":"task","root":true,"parents":["*"]});
    covered.push(check_callers(&w, &help, None, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Use configured work"}), &w.bot, agent_or_owner).await?);
    for id in ["a", "b", "c"] {
        w.command(json!({"op":"save_object","kind":"job","id":id,"revision":0,"parent":null,"data":{"title":"Implement","owner":"alex","contract":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
    }
    w.note("secret", 0, "Secret note", "Token s3cr3t", json!([]))
        .await?;
    covered.push(check_callers(&w, &help, None, json!({"op":"archive_object","target":"capability:empty","revision":revision("capability:empty").await?,"reason":"Hide the capability"}), &w.bot, agent_or_owner).await?);
    w.command(json!({"op":"archive_object","target":"note:secret","revision":revision("note:secret").await?,"reason":"Hide the note"})).await?;
    covered.push(check_callers(&w, &help, None, json!({"op":"restore_object","target":"note:secret","revision":revision("note:secret").await?,"reason":"Show the note"}), &w.bot, agent_or_owner).await?);
    covered.push(check_callers(&w, &help, None, json!({"op":"redact_object","target":"note:secret","fragment":"s3cr3t","reason":"Remove the secret"}), &w.owner, owner_only).await?);
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"rename_workspace","title":"Spec over HTTP again","reason":"Rename"}),
            &w.owner,
            "workspace owner renames the workspace",
        )
        .await?,
    );
    covered.push(check_callers(&w, &help, None, json!({"op":"reorder_tree","kind":"job","parent":null,"expected":["a","b","c"],"ids":["c","b","a"]}), &w.owner, "workspace owner reorders the tree").await?);
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"member","id":"newcomer","role":"human"}),
            &w.owner,
            owner_only,
        )
        .await?,
    );
    w.command(json!({"op":"domain","id":"spare","title":"Spare","owner":"owner"}))
        .await?;
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"set_domain_archived","id":"spare","archived":true,"reason":"Unused"}),
            &w.owner,
            "workspace owner archives domains",
        )
        .await?,
    );
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"confirm_tree"}),
            &w.owner,
            "workspace owner confirms the tree",
        )
        .await?,
    );
    covered.push(check_callers(&w, &help, None, json!({"op":"edit_domain","id":"security","title":"Security rules","owner":"owner","reason":"Clarify"}), &w.owner, "only the workspace owner edits confirmed domains").await?);
    covered.push(check_callers(&w, &help, None, json!({"op":"edit_capability","id":"restrict","title":"Restrict all access","reason":"Clarify"}), &w.owner, "only the workspace owner edits confirmed capabilities").await?);
    // alex owns each job. The Task owner is neither the holder nor the workspace owner.
    let bot = Some(w.bot.as_str());
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:a","revision":revision("job:a").await?}),
    )
    .await?;
    covered.push(check_callers(&w, &help, bot, json!({"op":"block_task","id":"job:a","revision":revision("job:a").await?,"reason":"The test fails","evidence":"test a failed at commit c1"}), &w.bot, "Task holder required").await?);
    covered.push(check_callers(&w, &help, None, json!({"op":"unblock_task","id":"job:a","revision":revision("job:a").await?,"reason":"The failure was outside the Task","evidence":"Run: r1"}), &w.owner, "planning holder or workspace owner required").await?);
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:b","revision":revision("job:b").await?}),
    )
    .await?;
    check_callers(&w, &help, bot, json!({"op":"block_task","id":"job:b","revision":revision("job:b").await?,"reason":"The test fails","evidence":"test b failed at commit c1"}), &w.owner, "Task holder required").await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:c","revision":revision("job:c").await?}),
    )
    .await?;
    covered.push(check_callers(&w, &help, bot, json!({"op":"release_task","id":"job:c","revision":revision("job:c").await?,"reason":"Return the work"}), &w.bot, owner_only).await?);
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:c","revision":revision("job:c").await?}),
    )
    .await?;
    covered.push(check_callers(&w, &help, bot, json!({"op":"complete_task","id":"job:c","revision":revision("job:c").await?,"evidence":"Implemented and checked"}), &w.bot, "Task is not claimed unfinished work").await?);
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command_as(&w.bot, json!({"op":"claim_planning","ac":"ac:key-rights","revision":revision("ac:key-rights").await?})).await?;
    covered.push(
        check_callers(
            &w,
            &help,
            bot,
            json!({"op":"release_planning","ac":"ac:key-rights","reason":"Plan later"}),
            &w.bot,
            owner_only,
        )
        .await?,
    );
    w.command_as(&w.bot, json!({"op":"claim_planning","ac":"ac:key-rights","revision":revision("ac:key-rights").await?})).await?;
    w.command_as(&w.bot, json!({"op":"save_object","kind":"task","id":"planned","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Plan the work"})).await?;
    covered.push(
        check_callers(
            &w,
            &help,
            bot,
            json!({"op":"finish_planning","ac":"ac:key-rights","reason":"The plan is complete"}),
            &w.bot,
            "planning holder required",
        )
        .await?,
    );
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"remove_member","id":"newcomer","reason":"Remove access"}),
            &w.owner,
            owner_only,
        )
        .await?,
    );
    // The deletion comes last: after it, the workspace accepts no command.
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"delete_workspace","reason":"Close the workspace"}),
            &w.owner,
            "workspace owner deletes the workspace",
        )
        .await?,
    );
    let mut restricted: Vec<String> = help["operations"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, entry)| {
            !entry["callers"]
                .as_array()
                .unwrap()
                .contains(&json!("member"))
        })
        .map(|(name, _)| name.clone())
        .collect();
    restricted.sort();
    covered.sort();
    ensure!(
        covered == restricted,
        "covered {covered:?} / restricted {restricted:?}"
    );
    Ok(())
}

// mod operation_samples is declared in tests/spec_http.rs, so its #[path] resolves from tests/.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn help_required_fields_pass_the_live_field_checks() -> Result<()> {
    let w = world().await?;
    let help = mcp_value(&w, "joppa_help", json!({})).await?;
    let required = |name: &str| -> Vec<String> {
        help["operations"][name]["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f.as_str().map(String::from))
            .collect()
    };
    // A ready criterion is the prerequisite state of a legacy Task.
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let task = json!({"op":"task","req":"key","revision":1,"title":"Build key rights","acs":["key-rights"],"projects":[],"purpose":"implementation","contract":"Implement and provide evidence"});
    let mut fields: Vec<String> = task
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| *k != "op")
        .cloned()
        .collect();
    fields.sort();
    let mut expected = required("task");
    expected.sort();
    ensure!(fields == expected, "task help fields {expected:?}");
    for field in &fields {
        let mut missing = task.clone();
        missing.as_object_mut().unwrap().remove(field);
        let (ok, reply) = send_as(&w, &w.owner, &missing).await?;
        ensure!(
            !ok && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("missing field")),
            "task without {field}: {reply}"
        );
    }
    let (ok, reply) = send_as(&w, &w.owner, &task).await?;
    ensure!(ok, "task from help fields: {reply}");
    // Every help entry: the complete call passes field validation; each removal fails it.
    for name in help["operations"].as_object().unwrap().keys() {
        let fields = required(name);
        let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
        let command = operation_samples::call(name, &fields);
        let (_, reply) = send_as(&w, &w.owner, &command).await?;
        ensure!(
            !reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("missing field")),
            "{name} from help fields: {reply}"
        );
        for field in &fields {
            let mut missing = command.clone();
            missing.as_object_mut().unwrap().remove(*field);
            let (ok, reply) = send_as(&w, &w.owner, &missing).await?;
            ensure!(
                !ok && reply["error"]
                    .as_str()
                    .is_some_and(|e| e.contains("missing field")),
                "{name} without {field}: {reply}"
            );
        }
    }
    Ok(())
}
