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

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn two_parallel_claims_give_exactly_one_winner() -> Result<()> {
    // atomic-claim-ac1
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
            &json!({"op":"member","id":bot,"role":"agent"}),
        )
        .await?;
    }
    let current = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut types = std::collections::BTreeMap::new();
    for (kind, role, root) in [("spec", "document", true), ("task", "task", false)] {
        types.insert(
            kind,
            TreeType {
                index: None,
                references: Default::default(),
                parent_decision: false,
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
        let (ra, rb) = (format!("a-{run}"), format!("b-{run}"));
        let (a, b) = tokio::join!(
            store::execute_remote_input(&pool, &ws, "bot1", &ra, &claim),
            store::execute_remote_input(&pool, &ws, "bot2", &rb, &claim)
        );
        let (won, lost) = match (a, b) {
            (Ok(won), Err(lost)) | (Err(lost), Ok(won)) => (won, lost),
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
        store::execute_remote_input(&pool, &ws, "owner", &format!("release-{run}"), &json!({"op":"release_task","id":address,"revision":current["document"]["revision"],"reason":"Reset the race fixture"})).await?;
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
        ("bot", json!({"op":"member","id":"bot","role":"agent"})),
        ("bot2", json!({"op":"member","id":"bot2","role":"agent"})),
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
