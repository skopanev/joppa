// Part 1 of tests/unstarted_work.rs, included there, so the test names stay unchanged.

async fn apply(pool: &Pool, ws: &str, actor: &str, command: Value) -> anyhow::Result<Value> {
    let request = uuid::Uuid::new_v4().to_string();
    if is_removed(&command) {
        append_legacy(pool, ws, actor, &request, &command).await?;
        return Ok(Value::Null);
    }
    store::execute_input(pool, ws, actor, &request, &command).await
}

async fn fixture() -> anyhow::Result<(Pool, String)> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok((pool, format!("planning-{}", uuid::Uuid::new_v4().simple())))
}

async fn candidates(pool: &Pool, ws: &str) -> anyhow::Result<Value> {
    store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            limit: 100,
            ..Default::default()
        },
    )
    .await
}

fn criteria(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            i["signal"]["assessment"]["criterion"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

async fn task_detail(pool: &Pool, ws: &str, address: &str) -> anyhow::Result<Value> {
    store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await
}

async fn assess_current(pool: &Pool, ws: &str, address: &str, ready: bool) -> anyhow::Result<()> {
    let obj = task_detail(pool, ws, address).await?;
    apply(pool, ws, "agent", json!({"op":"assess","req":obj["process"]["req"],"revision":obj["process"]["assessment"]["revision"],"ac":obj["process"]["criterion"],"assessment":{"ready":ready,"reason":if ready {"Contract is clear"} else {"Which behavior is required?"},"recipient":if ready {Value::Null} else {json!("owner")}}})).await?;
    Ok(())
}
