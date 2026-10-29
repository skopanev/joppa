// Part 1 of tests/board_field_postgres.rs, included there, so the test names stay unchanged.

const X: &str = "tests/app.rs > x";

const C1: &str = "c0ffee4";

const C2: &str = "c0ffee5";

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

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<Value> {
    store::execute_remote_input(pool, ws, "bot", &uuid::Uuid::new_v4().to_string(), &command).await
}

/// Requirement R with criterion A that lists X. The result shape has `shape`.
async fn fixture(pool: &deadpool_postgres::Pool, shape: Option<Value>) -> Result<String> {
    let ws = format!("board-field-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Board field"}),
    )
    .await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    let mut schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]}
    }});
    // The command sends the whole block: the current rules with the added fields.
    if let Some(shape) = shape {
        let mut block = tree["schema"]["test_results"].clone();
        let mut fields = block["fields"].as_array().cloned().unwrap_or_default();
        fields.extend(shape["fields"].as_array().cloned().unwrap_or_default());
        block["fields"] = json!(fields);
        schema["test_results"] = block;
    }
    write(pool, &ws, schema).await?;
    for command in [
        json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"}),
        json!({"op":"set_ac_tests","target":"ac:a","tests":[X],"reason":"Bind"}),
    ] {
        write(pool, &ws, command).await?;
    }
    Ok(ws)
}

fn app_shape() -> Value {
    json!({"fields":[{"name":"app","title":"App","type":"enum","values":["ios","android"],"required":true,"board":true}]})
}

async fn record(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    commit: &str,
    results: Value,
) -> Result<()> {
    store::record_results(pool, ws, "bot", &json!({"commit":commit,"results":results})).await?;
    Ok(())
}

/// The value tones of A and of R, from the stored rows.
async fn tones(pool: &deadpool_postgres::Pool, ws: &str) -> Result<(Value, Value)> {
    let read = |parent: Option<&'static str>| store::ObjectScope {
        parent,
        limit: 100,
        ..Default::default()
    };
    let criteria = store::read_objects(pool, ws, "bot", &read(Some("requirement:r"))).await?;
    let roots = store::read_objects(pool, ws, "bot", &read(None)).await?;
    let find = |list: &Value, address: &str| {
        list["items"]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["address"] == address))
            .map(|i| i["value_tones"].clone())
            .unwrap_or(Value::Null)
    };
    Ok((find(&criteria, "ac:a"), find(&roots, "requirement:r")))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_value_of_a_board_field_gets_its_own_status_and_tone() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool, Some(app_shape())).await?;
    let board = store::board_read(&pool, &ws, "bot").await?;
    ensure!(
        board["result_filters"]
            == json!([{"field":"app","title":"App","values":["ios","android"]}]),
        "{board}"
    );
    // Only ios has a result: android is grey, so A is grey.
    record(
        &pool,
        &ws,
        C1,
        json!([{"test":X,"status":"pass","app":"ios"}]),
    )
    .await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(
        s["status"] == "grey"
            && s["reason"] == "not every value passed"
            && s["values"]["app"]["ios"]["status"] == "green"
            && s["values"]["app"]["android"]["status"] == "grey",
        "{s}"
    );
    let (a, r) = tones(&pool, &ws).await?;
    ensure!(
        a == json!({"app:ios":"green","app:android":"grey"}),
        "A: {a}"
    );
    ensure!(
        r == json!({"app:ios":"green","app:android":"grey"}),
        "R: {r}"
    );
    // A later pass on ios does not hide the android failure.
    record(
        &pool,
        &ws,
        C1,
        json!([{"test":X,"status":"fail","app":"android","message":"crash"}]),
    )
    .await?;
    record(
        &pool,
        &ws,
        C2,
        json!([{"test":X,"status":"pass","app":"ios"}]),
    )
    .await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(
        s["status"] == "red"
            && s["tests"][0]["status"] == "fail"
            && s["tests"][0]["message"] == "crash"
            && s["values"]["app"]["ios"]["status"] == "green",
        "{s}"
    );
    let (a, _) = tones(&pool, &ws).await?;
    ensure!(
        a == json!({"app:ios":"green","app:android":"red"}),
        "A: {a}"
    );
    // A pass on android makes every value green.
    record(
        &pool,
        &ws,
        C2,
        json!([{"test":X,"status":"pass","app":"android"}]),
    )
    .await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(s["status"] == "green" && s["reason"].is_null(), "{s}");
    let (a, r) = tones(&pool, &ws).await?;
    ensure!(
        a == json!({"app:ios":"green","app:android":"green"}),
        "A: {a}"
    );
    ensure!(
        r == json!({"app:ios":"green","app:android":"green"}),
        "R: {r}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn without_a_board_field_the_status_and_rows_do_not_change() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool, None).await?;
    record(&pool, &ws, C1, json!([{"test":X,"status":"pass"}])).await?;
    let s = store::criterion_tests(&pool, &ws, "bot", "ac:a").await?;
    ensure!(s["status"] == "green" && s.get("values").is_none(), "{s}");
    let (a, r) = tones(&pool, &ws).await?;
    ensure!(a.is_null() && r.is_null(), "A: {a}, R: {r}");
    let board = store::board_read(&pool, &ws, "bot").await?;
    ensure!(board["result_filters"] == json!([]), "{board}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn only_an_enum_field_can_be_a_board_field() -> Result<()> {
    let pool = pool().await?;
    let ws = fixture(&pool, None).await?;
    let tree = store::tree_schema(&pool, &ws, "bot", None).await?;
    let mut block = tree["schema"]["test_results"].clone();
    block["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"app","type":"text","max_length":10,"board":true}));
    let error = write(&pool, &ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Board field","test_results":block}))
        .await
        .unwrap_err()
        .to_string();
    ensure!(
        error.contains("board field app must be an enum or ref field"),
        "{error}"
    );
    Ok(())
}
