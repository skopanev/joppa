// Part 1 of tests/criterion_reset_postgres.rs, included there, so the test names stay unchanged.

const A: &str = "tests/auth/login.test.ts > login > opens";

const B: &str = "tests/auth/login.test.ts > login > refuses";

async fn pool() -> Result<deadpool_postgres::Pool> {
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
    Ok(pool)
}

async fn write(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    command: Value,
) -> Result<Value> {
    // An agent or a runner enrolls through the internal event, as an agent token does.
    if command["op"] == "enroll_service" {
        return store::enroll_service(
            pool,
            ws,
            actor,
            command["id"].as_str().unwrap_or_default(),
            command["role"].as_str().unwrap_or_default(),
        )
        .await;
    }
    store::execute_remote_input(pool, ws, actor, &uuid::Uuid::new_v4().to_string(), &command).await
}

async fn read(pool: &deadpool_postgres::Pool, ws: &str, address: &str) -> Result<Value> {
    store::read_objects(
        pool,
        ws,
        "owner",
        &store::ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await
}

async fn status(pool: &deadpool_postgres::Pool, ws: &str, ac: &str) -> Result<Value> {
    store::test_status(pool, ws, "owner", ac).await
}

fn save_g(revision: &Value, body: &str) -> Value {
    json!({"op":"save_object","kind":"area","id":"g","revision":revision,"parent":null,"data":{"title":"G","body":body,"owner":"owner"},"reason":"Change the group"})
}

/// A test node that binds one test to a criterion.
fn bind(criterion: &str, test: &str) -> Value {
    json!({"op":"save_object","kind":"test","revision":0,"parent":criterion,"data":{"title":test,"owner":"owner"},"reason":"Bind the tests"})
}

/// G holds spec R with criterion A; sibling group H holds spec RH with criterion D.
/// Both have test nodes for A and B, and one record passes both.
async fn branches(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("reset-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Criterion reset"}),
    )
    .await?;
    write(
        pool,
        &ws,
        "owner",
        json!({"op":"enroll_service","id":"planner","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(pool, &ws, "owner", None).await?;
    write(pool, &ws, "owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure groups","types":{
        "area":{"title":"Area","role":"group","status":"children","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","status":"children","parents":["spec"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["criterion"],"title_preview_length":400}
    }})).await?;
    for (area, spec, criterion) in [("g", "r", "a"), ("h", "rh", "d")] {
        write(pool, &ws, "owner", json!({"op":"save_object","kind":"area","id":area,"revision":0,"parent":null,"data":{"title":area.to_uppercase(),"body":"Group scope","owner":"owner"},"reason":"Create the group"})).await?;
        write(pool, &ws, "owner", json!({"op":"save_object","kind":"spec","id":spec,"revision":0,"parent":format!("area:{area}"),"data":{"title":spec,"body":"Implement the rule","owner":"owner"},"reason":"Create the spec"})).await?;
        write(pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":criterion,"revision":0,"parent":format!("spec:{spec}"),"data":{"title":criterion,"owner":"owner"},"reason":"Create the criterion"})).await?;
        for test in [A, B] {
            write(
                pool,
                &ws,
                "planner",
                bind(&format!("criterion:{criterion}"), test),
            )
            .await?;
        }
    }
    upload_c1(pool, &ws).await?;
    for ac in ["criterion:a", "criterion:d"] {
        ensure!(
            status(pool, &ws, ac).await?["status"] == "green",
            "{ac} must start green"
        );
    }
    Ok(ws)
}

/// The C1 record that passes A and B.
async fn upload_c1(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    store::record_results(
        pool,
        ws,
        "planner",
        &json!({"commit":"c1c1c1c","results":[{"test":A,"status":"pass"},{"test":B,"status":"pass"}]}),
    )
    .await
}

/// The owner changes the text of criterion A and keeps its test nodes, so A is grey until a
/// newer record.
async fn change_a(pool: &deadpool_postgres::Pool, ws: &str) -> Result<()> {
    let a = read(pool, ws, "criterion:a").await?;
    write(pool, ws, "owner", json!({"op":"save_object","kind":"criterion","id":"a","revision":a["current_revision"],"parent":"spec:r","data":{"title":"a changed","owner":"owner"},"reason":"Sharper wording"})).await?;
    let s = status(pool, ws, "criterion:a").await?;
    ensure!(
        s["status"] == "grey" && s["tests"].as_array().is_some_and(|t| t.len() == 2),
        "after the change: {s}"
    );
    Ok(())
}

/// The ID of the C1 report and the number of its repeat receipts.
async fn c1_receipts(pool: &deadpool_postgres::Pool, ws: &str) -> Result<(i64, i64)> {
    let row = pool.get().await?.query_one("SELECT r.id,(SELECT count(*) FROM joppa.test_report_receipts c WHERE c.report_id=r.id) FROM joppa.test_reports r WHERE r.workspace=$1 AND r.commit_sha='c1c1c1c'", &[&ws]).await?;
    Ok((row.get(0), row.get(1)))
}

/// The state that replay must reproduce: statuses and stored boundaries.
async fn snapshot(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    let mut snapshot = json!({});
    for ac in ["criterion:a", "criterion:d"] {
        snapshot[ac] = json!({"status":status(pool, ws, ac).await?});
    }
    let boundaries: String = pool.get().await?.query_one("SELECT COALESCE(jsonb_object_agg(key,value),'{}')::text FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries'", &[&ws]).await?.get(0);
    snapshot["boundaries"] = serde_json::from_str(&boundaries)?;
    Ok(snapshot)
}
