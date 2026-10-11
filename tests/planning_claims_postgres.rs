use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn write(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    actor: &str,
    command: Value,
) -> Result<Value> {
    store::execute_remote_input(
        pool,
        workspace,
        actor,
        &uuid::Uuid::new_v4().to_string(),
        &command,
    )
    .await
}
async fn read(pool: &deadpool_postgres::Pool, workspace: &str, address: &str) -> Result<Value> {
    store::read_objects(
        pool,
        workspace,
        "owner",
        &store::ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await
}
async fn save_work(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    actor: &str,
    id: &str,
    revision: u32,
) -> Result<Value> {
    write(pool, workspace, actor, json!({"op":"save_object","kind":"work","id":id,"revision":revision,"parent":"criterion:a", "data":{"title":id,"owner":"owner","contract":format!("Implement the agreed mechanism revision {revision}"),"purpose":"implementation"},"reason":"Set the work contract"})).await
}

// planning-claim-ac1 through planning-claim-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_race_publication_release_and_replay_use_the_store() -> Result<()> {
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
    let workspace = format!("planning-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &workspace,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Planning claim checks"}),
    )
    .await?;
    for actor in ["planner", "other", "worker", "next-worker"] {
        write(
            &pool,
            &workspace,
            "owner",
            json!({"op":"member","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&pool, &workspace, "owner", None).await?;
    write(&pool, &workspace, "owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]},
        "work":{"title":"Work","role":"task","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    write(&pool, &workspace, "planner", json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":"owner"},"reason":"Set the rule"})).await?;
    for id in ["a", "b"] {
        write(&pool, &workspace, "planner", json!({"op":"save_object","kind":"criterion","id":id,"revision":0,"parent":"spec:one","data":{"title":id,"owner":"owner"},"reason":"Set the criterion"})).await?;
    }
    let req = read(&pool, &workspace, "spec:one").await?;
    let revision = req["process"]["revision"].as_u64().unwrap();
    write(
        &pool,
        &workspace,
        "owner",
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    )
    .await?;
    for ac in ["criterion:a", "criterion:b"] {
        write(&pool, &workspace, "planner", json!({"op":"assess","req":"spec:one","revision":revision,"ac":ac,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
    }
    save_work(&pool, &workspace, "planner", "old", 0).await?;
    let ac = read(&pool, &workspace, "criterion:a").await?;
    let command =
        json!({"op":"claim_planning","ac":"criterion:a","revision":ac["current_revision"]});
    let (left, right) = tokio::join!(
        write(&pool, &workspace, "planner", command.clone()),
        write(&pool, &workspace, "other", command)
    );
    let holder = match (left, right) {
        (Ok(_), Err(error)) => {
            assert_eq!(
                error.to_string(),
                "criterion is already claimed for planning"
            );
            "planner"
        }
        (Err(error), Ok(_)) => {
            assert_eq!(
                error.to_string(),
                "criterion is already claimed for planning"
            );
            "other"
        }
        (a, b) => anyhow::bail!("expected one planning holder, got {a:?} and {b:?}"),
    };
    save_work(&pool, &workspace, holder, "new", 0).await?;
    let all = store::task_queue(&pool, &workspace, "worker", "all", 100, None).await?;
    let new = all["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["address"] == "work:new")
        .unwrap();
    assert_eq!(new["status"], "open");
    assert_eq!(new["assignee"], Value::Null);
    assert_eq!(new["claimable"], false);
    assert_eq!(
        new["waiting_reason"],
        "Task is awaiting planning publication"
    );
    let direct = write(
        &pool,
        &workspace,
        "worker",
        json!({"op":"claim_task","id":"work:new","revision":1}),
    )
    .await
    .unwrap_err();
    assert_eq!(direct.to_string(), "Task is awaiting planning publication");
    let next = write(&pool, &workspace, "worker", json!({"op":"claim_next_task"})).await?;
    assert_eq!(next["command"]["id"], "work:old");
    let empty = write(
        &pool,
        &workspace,
        "next-worker",
        json!({"op":"claim_next_task"}),
    )
    .await
    .unwrap_err();
    assert_eq!(empty.to_string(), "no available Task matches this claim");

    // A held claim and its publication gate survive a complete projection rebuild.
    let held = read(&pool, &workspace, "criterion:a").await?;
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    store::migrate(&pool).await?;
    assert_eq!(
        read(&pool, &workspace, "criterion:a").await?["process"],
        held["process"]
    );
    let available =
        store::task_queue(&pool, &workspace, "next-worker", "available", 100, None).await?;
    assert!(available["tasks"].as_array().unwrap().is_empty());
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
    )
    .await?;
    assert!(read(&pool, &workspace, "criterion:a").await?["process"]["planning_claim"].is_null());
    let available =
        store::task_queue(&pool, &workspace, "next-worker", "available", 100, None).await?;
    assert_eq!(available["tasks"][0]["address"], "work:new");
    assert_eq!(
        read(&pool, &workspace, "work:old").await?["process"]["assignee"],
        "worker"
    );

    let ac = read(&pool, &workspace, "criterion:a").await?;
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"claim_planning","ac":"criterion:a","revision":ac["current_revision"]}),
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    save_work(
        &pool,
        &workspace,
        holder,
        "old",
        old["current_revision"].as_u64().unwrap() as u32,
    )
    .await?;
    save_work(&pool, &workspace, holder, "withdrawn", 0).await?;
    let preserved = read(&pool, &workspace, "work:old").await?;
    write(
        &pool,
        &workspace,
        "owner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Plan withdrawn"}),
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    assert_eq!(old["document"], preserved["document"]);
    assert_eq!(old["process"]["assignee"], "worker");
    let retired = read(&pool, &workspace, "work:withdrawn").await?;
    assert_eq!(retired["process"]["status"], "retired");
    assert_eq!(retired["process"]["retired"], "Plan withdrawn");
    assert_eq!(retired["document"]["present"], true);

    // A worker revision must not erase the planner's update-only finish evidence.
    let ac = read(&pool, &workspace, "criterion:a").await?;
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"claim_planning","ac":"criterion:a","revision":ac["current_revision"]}),
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    save_work(
        &pool,
        &workspace,
        holder,
        "old",
        old["current_revision"].as_u64().unwrap() as u32,
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    write(&pool, &workspace, "worker", json!({"op":"release_task","id":"work:old","revision":old["current_revision"],"reason":"Return existing work"})).await?;
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Update complete"}),
    )
    .await?;
    store::check_journal(&pool).await?;
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    store::migrate(&pool).await?;
    assert_eq!(
        read(&pool, &workspace, "work:withdrawn").await?["process"]["status"],
        "retired"
    );
    assert!(read(&pool, &workspace, "criterion:a").await?["process"]["planning_claim"].is_null());
    store::check_journal(&pool).await?;
    Ok(())
}
