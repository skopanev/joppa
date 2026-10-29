// Part 1 of tests/claims_postgres.rs, included there, so the test names stay unchanged.

struct Server {
    child: std::process::Child,
    dir: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// [ac:atomic-claim-ac1]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn two_parallel_claims_give_exactly_one_winner() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("claims-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Claims"}),
    )
    .await?;
    // alex and maria are two different human members.
    for member in ["alex", "maria"] {
        store::execute_remote_input(
            &pool,
            &ws,
            "owner",
            member,
            &json!({"op":"member","id":member,"role":"human"}),
        )
        .await?;
    }
    let current = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut types = std::collections::BTreeMap::new();
    for (kind, role, root) in [("spec", "document", true), ("task", "task", false)] {
        types.insert(
            kind,
            TreeType {
                question: None,
                index: None,
                references: Default::default(),
                required_fields: Default::default(),
                parent_decision: false,
                status: Some("none".into()),
                status_order: None,
                process_fields: None,
                inbox: None,
                launch_blocker: None,
                color: None,
                title_size: None,
                badge: None,
                title_preview_length: None,
                title: kind.into(),
                role: role.into(),
                root,
                parents: vec!["*".into()],
                parent_gate: None,
            },
        );
    }
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":current["schema"]["revision"],"types":types,"reason":"Specifications and tasks only"})).await?;
    store::execute_remote_input(&pool,&ws,"owner","root",&json!({"op":"save_object","kind":"spec","id":"root","revision":0,"parent":null,"data":{"title":"root","owner":"owner"},"reason":"Create a specification"})).await?;
    for run in 0..50 {
        let task = store::execute_remote_input(&pool,&ws,"owner",&format!("task-{run}"),&json!({"op":"save_object","kind":"task","revision":0,"parent":"spec:root","data":{"title":"Implement","owner":"owner","contract":"Implement the specification","purpose":"implementation","projects":[]},"reason":"Create work"})).await?;
        let claim = json!({"op":"claim_task","id":task["object"]["address"],"revision":task["object"]["revision"]});
        for member in ["alex", "maria"] {
            let mine = store::task_queue(&pool, &ws, member, "mine", 10, None).await?;
            ensure!(
                mine["total"] == 0,
                "run {run}: {member} mine total {}",
                mine["total"]
            );
        }
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let [a, b] = [("alex", format!("a-{run}")), ("maria", format!("b-{run}"))].map(
            |(member, request)| {
                let (pool, ws, claim, barrier) =
                    (pool.clone(), ws.clone(), claim.clone(), barrier.clone());
                tokio::spawn(async move {
                    barrier.wait().await;
                    store::execute_remote_input(&pool, &ws, member, &request, &claim).await
                })
            },
        );
        let (a, b) = (a.await, b.await);
        let (winner, won, lost) = match (a, b) {
            (Ok(Ok(won)), Ok(Err(lost))) => ("alex", won, lost),
            (Ok(Err(lost)), Ok(Ok(won))) => ("maria", won, lost),
            (a, b) => anyhow::bail!("run {run}: {a:?} / {b:?}"),
        };
        ensure!(won["command"]["op"] == "claim_task", "run {run}: {won}");
        ensure!(
            format!("{lost:#}").contains("task is already claimed"),
            "run {run}: {lost:#}"
        );
        let address = task["object"]["address"].as_str().unwrap();
        let current = store::read_objects(
            &pool,
            &ws,
            "owner",
            &store::ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await?;
        ensure!(
            current["process"]["assignee"] == winner,
            "run {run}: winner {winner}, assignee {}",
            current["process"]["assignee"]
        );
        ensure!(
            current["process"]["status"] == "in_progress",
            "run {run}: status {}",
            current["process"]["status"]
        );
        store::execute_remote_input(&pool, &ws, "owner", &format!("release-{run}"), &json!({"op":"release_task","id":address,"revision":current["document"]["revision"],"reason":"Reset the race fixture"})).await?;
        let released = store::read_objects(
            &pool,
            &ws,
            "owner",
            &store::ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await?;
        ensure!(
            released["process"]["assignee"].is_null(),
            "run {run}: assignee after release {}",
            released["process"]["assignee"]
        );
        let mine = store::task_queue(&pool, &ws, winner, "mine", 10, None).await?;
        ensure!(
            mine["total"] == 0,
            "run {run}: {winner} mine total after release {}",
            mine["total"]
        );
    }
    Ok(())
}
