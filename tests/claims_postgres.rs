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
                question: None,
                index: None,
                references: Default::default(),
                required_fields: Default::default(),
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

#[path = "common/claim_time.rs"]
mod claim_time;

async fn heartbeat_fixture() -> Result<(deadpool_postgres::Pool, String)> {
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
    let ws = format!("heartbeat-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Heartbeat"}),
    )
    .await?;
    for actor in ["bot", "other"] {
        store::execute_input(
            &pool,
            &ws,
            "owner",
            actor,
            &json!({"op":"member","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":{"job":{"title":"Jobs","role":"task","root":true,"parents":["*"]}},"reason":"Use a configured work type"})).await?;
    store::execute_remote_input(&pool,&ws,"owner","job",&json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement the contract","owner":"owner","contract":"Implement and check the behavior","purpose":"implementation"},"reason":"Create work"})).await?;
    store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "claim",
        &json!({"op":"claim_task","id":"job:one","revision":1}),
    )
    .await?;
    Ok((pool, ws))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_expired_claim_returns_a_configured_task_to_the_queue() -> Result<()> {
    // heartbeat-return-ac1
    let (pool, ws) = heartbeat_fixture().await?;
    claim_time::age(&pool, &ws, "job:one", 31).await?;
    let queue = store::task_queue(&pool, &ws, "bot", "available", 10, None).await?;
    ensure!(
        queue["tasks"][0]["status"] == "open"
            && queue["tasks"][0]["assignee"].is_null()
            && queue["tasks"][0]["claimable"] == true,
        "{queue}"
    );
    ensure!(
        store::task_queue(&pool, &ws, "bot", "mine", 10, None).await?["total"] == 0,
        "expired hold is not mine"
    );
    for command in [
        json!({"op":"heartbeat_task","id":"job:one","revision":2}),
        json!({"op":"complete_task","id":"job:one","revision":2,"evidence":"Old claim"}),
    ] {
        ensure!(
            store::execute_remote_input(
                &pool,
                &ws,
                "bot",
                &uuid::Uuid::new_v4().to_string(),
                &command
            )
            .await
            .is_err(),
            "expired holder must fail"
        );
    }
    let claim = json!({"op":"claim_task","id":"job:one","revision":2});
    let (a, b) = tokio::join!(
        store::execute_remote_input(&pool, &ws, "bot", "race-a", &claim),
        store::execute_remote_input(&pool, &ws, "other", "race-b", &claim)
    );
    ensure!(
        a.is_ok() != b.is_ok(),
        "exactly one claim must win: {a:?} {b:?}"
    );
    let object = store::read_objects(
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
        object["current_revision"] == 3 && object["process"]["status"] == "in_progress",
        "{object}"
    );
    let client = pool.get().await?;
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM joppa.object_revisions WHERE workspace=$1 AND address='job:one'",
            &[&ws],
        )
        .await?
        .get(0);
    ensure!(count == 3, "claim history remains");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_claim_after_lock_wait_uses_event_time_and_replays() -> Result<()> {
    let (pool, ws) = heartbeat_fixture().await?;
    claim_time::age_ms(&pool, &ws, "job:one", 30 * 60_000 - 3_000).await?;
    let mut blocker = pool.get().await?;
    let lock = blocker.transaction().await?;
    lock.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&ws],
    )
    .await?;
    let deadline: i64 = lock.query_one("SELECT (process->'claim'->>'deadline_ms')::bigint FROM joppa.objects WHERE workspace=$1 AND address='job:one'", &[&ws]).await?.get(0);
    let claim_pool = pool.clone();
    let claim_ws = ws.clone();
    let pending = tokio::spawn(async move {
        store::execute_remote_input(
            &claim_pool,
            &claim_ws,
            "other",
            "claim-after-wait",
            &json!({"op":"claim_next_task"}),
        )
        .await
    });
    let observer = pool.get().await?;
    let mut started_before_deadline = false;
    for _ in 0..100 {
        let waiting: bool = observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE l.locktype='advisory' AND NOT l.granted AND a.datname=current_database() AND floor(extract(epoch FROM a.xact_start)*1000)::bigint < $1)", &[&deadline]).await?.get(0);
        if waiting {
            started_before_deadline = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    ensure!(
        started_before_deadline,
        "claim transaction must begin before its deadline"
    );
    let now: i64 = lock
        .query_one(
            "SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint",
            &[],
        )
        .await?
        .get(0);
    if now <= deadline {
        tokio::time::sleep(std::time::Duration::from_millis(
            (deadline - now + 50) as u64,
        ))
        .await;
    }
    lock.commit().await?;
    drop(observer);
    let result = pending.await??;
    ensure!(result["command"]["id"] == "job:one", "{result}");
    let recorded: i64 = blocker.query_one("SELECT floor(extract(epoch FROM recorded_at)*1000)::bigint FROM joppa.events WHERE workspace=$1 AND request_id='claim-after-wait'", &[&ws]).await?.get(0);
    ensure!(
        recorded >= deadline,
        "event time must follow the expired deadline"
    );
    let heartbeat: i64 = blocker.query_one("SELECT (process->'claim'->>'heartbeat_at_ms')::bigint FROM joppa.objects WHERE workspace=$1 AND address='job:one'", &[&ws]).await?.get(0);
    ensure!(
        heartbeat == recorded,
        "live claim and replay use the same event time"
    );
    drop(blocker);
    store::check_journal(&pool).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn heartbeats_keep_a_claim_after_its_initial_deadline() -> Result<()> {
    // heartbeat-return-ac2
    let (pool, ws) = heartbeat_fixture().await?;
    let initial = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    for minute in (5..=30).step_by(5) {
        claim_time::age(&pool, &ws, "job:one", 5).await?;
        let heartbeat = json!({"op":"heartbeat_task","id":"job:one","revision":2});
        let request = format!("heartbeat-{minute}");
        let accepted = store::execute_remote_input(&pool, &ws, "bot", &request, &heartbeat).await?;
        let replayed = store::execute_remote_input(&pool, &ws, "bot", &request, &heartbeat).await?;
        ensure!(
            replayed["replayed"] == true && accepted["position"] == replayed["position"],
            "retry remains idempotent"
        );
        let current = store::read_objects(
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
            current["document"] == initial["document"]
                && current["process"]["origin_revision"] == initial["process"]["origin_revision"],
            "heartbeat preserves document and context: {current}"
        );
        ensure!(
            current["process"]["claim"]["deadline_ms"].as_i64().unwrap()
                - current["process"]["claim"]["heartbeat_at_ms"]
                    .as_i64()
                    .unwrap()
                == 30 * 60_000,
            "renew for 30 minutes"
        );
    }
    claim_time::age(&pool, &ws, "job:one", 1).await?;
    let queue = store::task_queue(&pool, &ws, "bot", "mine", 10, None).await?;
    ensure!(
        queue["tasks"][0]["status"] == "in_progress" && queue["tasks"][0]["assignee"] == "bot",
        "{queue}"
    );
    ensure!(
        store::execute_remote_input(
            &pool,
            &ws,
            "other",
            "steal",
            &json!({"op":"claim_task","id":"job:one","revision":2})
        )
        .await
        .is_err(),
        "another agent cannot claim"
    );
    ensure!(
        store::execute_remote_input(
            &pool,
            &ws,
            "other",
            "heartbeat-other",
            &json!({"op":"heartbeat_task","id":"job:one","revision":2})
        )
        .await
        .is_err(),
        "another agent cannot renew"
    );
    ensure!(
        store::execute_remote_input(
            &pool,
            &ws,
            "bot",
            "heartbeat-stale",
            &json!({"op":"heartbeat_task","id":"job:one","revision":1})
        )
        .await
        .is_err(),
        "stale revision cannot renew"
    );
    store::execute_remote_input(&pool,&ws,"bot","done",&json!({"op":"complete_task","id":"job:one","revision":2,"evidence":"Implemented and checked"})).await?;
    let done = store::read_objects(
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
        done["process"]["claim"].is_null(),
        "completed work has no active deadline"
    );
    ensure!(
        store::execute_remote_input(
            &pool,
            &ws,
            "bot",
            "heartbeat-done",
            &json!({"op":"heartbeat_task","id":"job:one","revision":3})
        )
        .await
        .is_err(),
        "completed work cannot renew"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn expiry_keeps_dependencies_and_cannot_restore_an_old_holder() -> Result<()> {
    let (pool, ws) = heartbeat_fixture().await?;
    store::execute_remote_input(&pool,&ws,"owner","dependency",&json!({"op":"save_object","kind":"job","id":"dependency","revision":0,"parent":null,"data":{"title":"Prerequisite","owner":"owner","contract":"Deliver the prerequisite","purpose":"implementation"},"reason":"Create dependency"})).await?;
    store::execute_remote_input(&pool,&ws,"owner","link",&json!({"op":"save_object","kind":"job","id":"one","revision":2,"parent":null,"data":{"title":"Implement the contract","owner":"owner","contract":"Implement and check the behavior","purpose":"implementation"},"links":[{"target":"job:dependency","relation":"depends_on","blocking":true,"revision":1}],"reason":"Work requires the prerequisite"})).await?;
    claim_time::age(&pool, &ws, "job:one", 31).await?;
    let waiting = store::task_queue(&pool, &ws, "bot", "waiting", 10, None).await?;
    ensure!(
        waiting["tasks"][0]["address"] == "job:one"
            && waiting["tasks"][0]["claimable"] == false
            && waiting["tasks"][0]["assignee"].is_null(),
        "{waiting}"
    );
    ensure!(
        waiting["tasks"][0]["signal"]["tone"] == "purple",
        "keep the dependency signal"
    );
    ensure!(
        store::execute_remote_input(
            &pool,
            &ws,
            "bot",
            "blocked-claim",
            &json!({"op":"claim_task","id":"job:one","revision":3})
        )
        .await
        .is_err(),
        "expiry cannot bypass a dependency"
    );
    store::execute_remote_input(
        &pool,
        &ws,
        "owner",
        "retire",
        &json!({"op":"retire_task","id":"job:one","revision":3,"reason":"Retire work"}),
    )
    .await?;
    let retired = store::read_objects(
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
        retired["process"]["status"] == "retired" && retired["process"]["claim"].is_null(),
        "{retired}"
    );
    ensure!(
        store::execute_remote_input(
            &pool,
            &ws,
            "bot",
            "heartbeat-retired",
            &json!({"op":"heartbeat_task","id":"job:one","revision":4})
        )
        .await
        .is_err(),
        "retired work cannot renew"
    );
    let (pool, ws) = heartbeat_fixture().await?;
    claim_time::age(&pool, &ws, "job:one", 31).await?;
    let heartbeat = json!({"op":"heartbeat_task","id":"job:one","revision":2});
    let claim = json!({"op":"claim_task","id":"job:one","revision":2});
    let (renew, reclaim) = tokio::join!(
        store::execute_remote_input(&pool, &ws, "bot", "late-heartbeat", &heartbeat),
        store::execute_remote_input(&pool, &ws, "other", "reclaim", &claim)
    );
    ensure!(
        renew.is_err() && reclaim.is_ok(),
        "an expired heartbeat cannot restore the holder: {renew:?} {reclaim:?}"
    );
    ensure!(
        store::execute_remote_input(
            &pool,
            &ws,
            "bot",
            "old-holder",
            &json!({"op":"heartbeat_task","id":"job:one","revision":3})
        )
        .await
        .is_err(),
        "old holder cannot renew with a fresh revision"
    );
    Ok(())
}
