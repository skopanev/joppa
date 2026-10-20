//! Parallel claims on the real store: one task has one winner.
use anyhow::{Result, ensure};
use joppa::{objects::TreeType, store};
use serde_json::json;

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
    for bot in ["bot1", "bot2"] {
        store::execute_input(
            &pool,
            &ws,
            "owner",
            bot,
            &json!({"op":"enroll_service","id":bot,"role":"agent"}),
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
                status: None,
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
        for bot in ["bot1", "bot2"] {
            let mine = store::task_queue(&pool, &ws, bot, "mine", 10, None).await?;
            ensure!(
                mine["total"] == 0,
                "run {run}: {bot} mine total {}",
                mine["total"]
            );
        }
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let [a, b] =
            [("bot1", format!("a-{run}")), ("bot2", format!("b-{run}"))].map(|(bot, request)| {
                let (pool, ws, claim, barrier) =
                    (pool.clone(), ws.clone(), claim.clone(), barrier.clone());
                tokio::spawn(async move {
                    barrier.wait().await;
                    store::execute_remote_input(&pool, &ws, bot, &request, &claim).await
                })
            });
        let (a, b) = (a.await, b.await);
        let (winner, won, lost) = match (a, b) {
            (Ok(Ok(won)), Ok(Err(lost))) => ("bot1", won, lost),
            (Ok(Err(lost)), Ok(Ok(won))) => ("bot2", won, lost),
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

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_agent_cannot_claim_two_tasks_even_in_parallel() -> Result<()> {
    // one-task-per-agent-ac1
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("single-claim-{}", uuid::Uuid::new_v4().simple());
    for (id, command) in [
        (
            "workspace",
            json!({"op":"workspace","title":"One active task per agent"}),
        ),
        (
            "bot",
            json!({"op":"enroll_service","id":"bot","role":"agent"}),
        ),
        (
            "bot2",
            json!({"op":"enroll_service","id":"bot2","role":"agent"}),
        ),
    ] {
        store::execute_input(&pool, &ws, "owner", id, &command).await?;
    }
    let current = store::tree_schema(&pool, &ws, "owner", None).await?;
    let types = json!({
        "spec":{"title":"Specs","role":"document","root":true,"parents":["*"]},
        "job":{"title":"Jobs","role":"task","root":true,"parents":["*"]},
        "work":{"title":"Work","role":"task","root":true,"parents":["*"]}
    });
    store::execute_remote_input(&pool, &ws, "owner", "schema", &json!({"op":"set_tree_schema","revision":current["schema"]["revision"],"types":types,"reason":"Use two configured work types"})).await?;
    for kind in ["job", "work"] {
        store::execute_remote_input(&pool, &ws, "owner", kind, &json!({"op":"save_object","kind":kind,"id":"one","revision":0,"parent":null,"data":{"title":"Implement the contract","owner":"owner","contract":"Implement and test the specified behavior","purpose":"implementation","projects":[]},"reason":"Create work"})).await?;
    }
    let claim_a = json!({"op":"claim_task","id":"job:one","revision":1});
    let claim_b = json!({"op":"claim_task","id":"work:one","revision":1});
    let (a, b) = tokio::join!(
        store::execute_remote_input(&pool, &ws, "bot", "claim-a", &claim_a),
        store::execute_remote_input(&pool, &ws, "bot", "claim-b", &claim_b)
    );
    let (held, other, error) = match (a, b) {
        (Ok(_), Err(e)) => ("job:one", "work:one", e),
        (Err(e), Ok(_)) => ("work:one", "job:one", e),
        (a, b) => anyhow::bail!("one claim must succeed: {a:?} / {b:?}"),
    };
    ensure!(
        error.to_string() == format!("agent already holds task {held}"),
        "{error:#}"
    );
    let mine = store::task_queue(&pool, &ws, "bot", "mine", 100, None).await?;
    ensure!(mine["tasks"].as_array().unwrap().len() == 1, "{mine}");
    let available = store::task_queue(&pool, &ws, "bot", "available", 100, None).await?;
    ensure!(
        available["tasks"].as_array().unwrap().is_empty(),
        "{available}"
    );
    let waiting = store::task_queue(&pool, &ws, "bot", "waiting", 100, None).await?;
    ensure!(waiting["tasks"][0]["address"] == other, "{waiting}");
    ensure!(waiting["tasks"][0]["claimable"] == false, "{waiting}");
    ensure!(
        waiting["tasks"][0]["waiting_reason"] == format!("agent already holds task {held}"),
        "{waiting}"
    );
    let dir = std::env::temp_dir().join(format!(
        "joppa-claim-http-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({token.clone():"bot"}).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let _server = Server {
        child: std::process::Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env(
                "JOPPA_DATABASE_URL",
                std::env::var("JOPPA_TEST_DATABASE_URL")?,
            )
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env_remove("DEEPINFRA_API_KEY")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?,
        dir,
    };
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let response = reqwest::Client::new().post(format!("{root}/api/workspaces/{ws}/commands"))
        .bearer_auth(&token)
        .json(&json!({"request_id":"http-refused-claim","command":{"op":"claim_task","id":other,"revision":1}}))
        .send().await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        response.status()
    );
    let response: serde_json::Value = response.json().await?;
    ensure!(
        response["error"] == format!("agent already holds task {held}"),
        "{response}"
    );
    let before = store::read_objects(
        &pool,
        &ws,
        "bot",
        &store::ObjectScope {
            address: Some(held),
            ..Default::default()
        },
    )
    .await?;
    let next = store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "next-while-held",
        &json!({"op":"claim_next_task"}),
    )
    .await
    .unwrap_err();
    ensure!(
        next.to_string() == "no available Task matches this claim",
        "{next:#}"
    );
    let after = store::read_objects(
        &pool,
        &ws,
        "bot",
        &store::ObjectScope {
            address: Some(held),
            ..Default::default()
        },
    )
    .await?;
    ensure!(
        before["position"] == after["position"],
        "a refused next must not write an event"
    );
    store::execute_remote_input(
        &pool,
        &ws,
        "bot2",
        "other-agent",
        &json!({"op":"claim_task","id":other,"revision":1}),
    )
    .await?;
    store::execute_remote_input(&pool, &ws, "bot", "release", &json!({"op":"release_task","id":held,"revision":before["document"]["revision"],"reason":"Return the work"})).await?;
    let available = store::task_queue(&pool, &ws, "bot", "available", 100, None).await?;
    ensure!(
        available["tasks"].as_array().unwrap().len() == 1
            && available["tasks"][0]["address"] == held,
        "{available}"
    );
    let next = store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "next-after-release",
        &json!({"op":"claim_next_task"}),
    )
    .await?;
    ensure!(next["command"]["id"] == held, "{next}");
    Ok(())
}

#[path = "common/claim_time.rs"]
mod claim_time;

async fn permanent_fixture(minutes: i64) -> Result<(deadpool_postgres::Pool, String)> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    claim_time::install(&pool).await?;
    let ws = format!("permanent-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Permanent claims"}),
    )
    .await?;
    for actor in ["bot", "other"] {
        store::execute_input(
            &pool,
            &ws,
            "owner",
            actor,
            &json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":{"job":{"title":"Jobs","role":"task","root":true,"parents":["*"]}},"reason":"Use configured work"})).await?;
    store::execute_remote_input(&pool,&ws,"owner","job",&json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement the contract","owner":"owner","contract":"Implement and verify the behavior","purpose":"implementation"},"reason":"Create work"})).await?;
    let request = format!(
        "test-claim-time-{}-{}",
        minutes * 60_000,
        uuid::Uuid::new_v4().simple()
    );
    store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        &request,
        &json!({"op":"claim_task","id":"job:one","revision":1}),
    )
    .await?;
    Ok((pool, ws))
}

// [ac:heartbeat-return-ac1]
// [ac:claims-persist-replay]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn elapsed_time_keeps_claims_in_current_queues_and_replay() -> Result<()> {
    for minutes in [31, 120, 1440] {
        let (pool, ws) = permanent_fixture(minutes).await?;
        let before = store::read_objects(
            &pool,
            &ws,
            "owner",
            &store::ObjectScope {
                address: Some("job:one"),
                ..Default::default()
            },
        )
        .await?;
        ensure!(
            before["process"]["status"] == "in_progress" && before["process"]["assignee"] == "bot",
            "{before}"
        );
        ensure!(
            store::task_queue(&pool, &ws, "bot", "mine", 10, None).await?["total"] == 1,
            "holder stays in mine"
        );
        ensure!(
            store::task_queue(&pool, &ws, "other", "available", 10, None).await?["total"] == 0,
            "claim stays unavailable"
        );
        let command = json!({"op":"claim_task","id":"job:one","revision":2});
        let refused = store::execute_remote_input(&pool, &ws, "other", "refused", &command)
            .await
            .unwrap_err();
        ensure!(
            refused.to_string() == "task is already claimed",
            "{refused}"
        );
        let after = store::read_objects(
            &pool,
            &ws,
            "owner",
            &store::ObjectScope {
                address: Some("job:one"),
                ..Default::default()
            },
        )
        .await?;
        ensure!(before == after, "refusal changes no state");
        store::execute_remote_input(&pool,&ws,"bot","done",&json!({"op":"complete_task","id":"job:one","revision":2,"evidence":"Verified after elapsed time"})).await?;
        store::check_journal(&pool).await?;
    }
    Ok(())
}

// [ac:heartbeat-return-ac2]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn heartbeat_refusal_preserves_documents_and_position() -> Result<()> {
    let (pool, ws) = permanent_fixture(1440).await?;
    let before = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    let error = store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "renew",
        &json!({"op":"heartbeat_task","id":"job:one","revision":2}),
    )
    .await
    .unwrap_err();
    ensure!(
        error.to_string() == "heartbeat_task is no longer supported",
        "{error}"
    );
    let after = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    ensure!(before == after, "refusal changes no state");
    Ok(())
}

// [ac:owner-releases-claim-ac1]
// [ac:old-agent-locked-out-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn explicit_owner_release_keeps_work_and_excludes_the_old_holder() -> Result<()> {
    let (pool, ws) = permanent_fixture(1440).await?;
    let before = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    store::execute_remote_input(
        &pool,
        &ws,
        "owner",
        "release",
        &json!({"op":"release_task","id":"job:one","revision":2,"reason":"Worker unavailable"}),
    )
    .await?;
    let released = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    ensure!(
        released["document"]["data"] == before["document"]["data"],
        "release keeps the document"
    );
    ensure!(
        store::task_queue(&pool, &ws, "other", "available", 10, None).await?["total"] == 1,
        "explicit release returns work"
    );
    store::execute_remote_input(
        &pool,
        &ws,
        "other",
        "claim",
        &json!({"op":"claim_task","id":"job:one","revision":3}),
    )
    .await?;
    let error = store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "old-holder",
        &json!({"op":"complete_task","id":"job:one","revision":4,"evidence":"Late completion"}),
    )
    .await
    .unwrap_err();
    ensure!(
        error.to_string() == "Task is not claimed unfinished work",
        "{error}"
    );
    store::check_journal(&pool).await?;
    Ok(())
}
