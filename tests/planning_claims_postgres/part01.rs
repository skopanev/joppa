// Part 1 of tests/planning_claims_postgres.rs, included there, so the test names stay unchanged.

async fn write(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    actor: &str,
    command: Value,
) -> Result<Value> {
    // An agent or a runner enrolls through the internal event, as an agent token does.
    if command["op"] == "enroll_service" {
        return store::enroll_service(
            pool,
            workspace,
            actor,
            command["id"].as_str().unwrap_or_default(),
            command["role"].as_str().unwrap_or_default(),
        )
        .await;
    }
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
    write(pool, workspace, actor, json!({"op":"save_object","kind":"work","id":id,"revision":revision,"parent":"criterion:a", "data":{"title":id,"owner":"owner","body":format!("Implement the agreed mechanism revision {revision}"),"purpose":"implementation"},"reason":"Set the work contract"})).await
}

/// Creates a workspace with ready, unverified criteria A and B under one confirmed spec.
/// Each actor is a human member, as the planners P and Q are people.
async fn planning_workspace(pool: &deadpool_postgres::Pool, actors: &[&str]) -> Result<String> {
    let workspace = format!("planning-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &workspace,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Planning claim checks"}),
    )
    .await?;
    for actor in actors {
        write(
            pool,
            &workspace,
            "owner",
            json!({"op":"member","id":actor,"role":"human"}),
        )
        .await?;
    }
    let schema = store::tree_schema(pool, &workspace, "owner", None).await?;
    write(pool, &workspace, "owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    write(pool, &workspace, "planner", json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":"owner"},"reason":"Set the rule"})).await?;
    for id in ["a", "b"] {
        write(pool, &workspace, "planner", json!({"op":"save_object","kind":"criterion","id":id,"revision":0,"parent":"spec:one","data":{"title":id,"owner":"owner"},"reason":"Set the criterion"})).await?;
    }
    let req = read(pool, &workspace, "spec:one").await?;
    let revision = req["process"]["revision"].as_u64().unwrap();
    write(
        pool,
        &workspace,
        "owner",
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    )
    .await?;
    for ac in ["criterion:a", "criterion:b"] {
        write(pool, &workspace, "planner", json!({"op":"assess","req":"spec:one","revision":revision,"ac":ac,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
    }
    Ok(workspace)
}
