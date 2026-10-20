//! An identical retry keeps its receipt, an uninvited account cannot write, and a report
//! upload updates the stored status of a criterion and the counters of its ancestors.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

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
    request_id: &str,
    command: &Value,
) -> Result<Value> {
    store::execute_remote_input(pool, ws, actor, request_id, command).await
}

async fn head(pool: &deadpool_postgres::Pool, ws: &str) -> Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0))
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

/// A workspace with the human members alex and maria and a four-level tree.
async fn workspace(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("access-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Access and retries"}),
    )
    .await?;
    for id in ["alex", "maria"] {
        let member = json!({"op":"member","id":id,"role":"human"});
        write(
            pool,
            &ws,
            "owner",
            &uuid::Uuid::new_v4().to_string(),
            &member,
        )
        .await?;
    }
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    let schema = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "domain":{"title":"Domains","role":"group","root":true,"parents":[]},
        "capability":{"title":"Capabilities","role":"group","parents":["domain"]},
        "requirement":{"title":"Requirements","role":"requirement","parents":["capability"]},
        "ac":{"title":"Criteria","role":"ac","parents":["requirement"]}
    }});
    write(
        pool,
        &ws,
        "owner",
        &uuid::Uuid::new_v4().to_string(),
        &schema,
    )
    .await?;
    Ok(ws)
}

fn save(kind: &str, id: &str, parent: Option<&str>, title: &str) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":title,"owner":"owner","body":"The rule"},"reason":"Create"})
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_identical_retry_keeps_the_receipt_and_a_mismatch_changes_nothing() -> Result<()> {
    // ac:idempotent-request-id-ac1
    let pool = pool().await?;
    let ws = workspace(&pool).await?;
    write(
        &pool,
        &ws,
        "alex",
        "domain",
        &save("domain", "d", None, "D"),
    )
    .await?;
    let p = head(&pool, &ws).await?;
    let original = save("domain", "r", None, "R");
    let first = write(&pool, &ws, "alex", "req-1", &original).await?;
    ensure!(head(&pool, &ws).await? == p + 1, "{first}");
    let retry = write(&pool, &ws, "alex", "req-1", &original).await?;
    ensure!(
        retry["position"] == first["position"] && retry["object"] == first["object"],
        "{first} {retry}"
    );
    ensure!(head(&pool, &ws).await? == p + 1, "the retry adds no event");
    let state = read(&pool, &ws, "domain:r").await?;
    ensure!(state["current_revision"] == 1, "{state}");
    let changed = save("domain", "r", None, "R changed");
    for (actor, command) in [("alex", &changed), ("maria", &original)] {
        write(&pool, &ws, actor, "req-1", command)
            .await
            .expect_err("a mismatched retry must be refused");
        ensure!(
            head(&pool, &ws).await? == p + 1,
            "{actor}: the journal grew"
        );
        let now = read(&pool, &ws, "domain:r").await?;
        ensure!(
            now["current_revision"] == 1 && now["document"] == state["document"],
            "{actor}: {now}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_uninvited_account_cannot_write_to_a_workspace() -> Result<()> {
    // ac:8b1b95c7: eve is neither the owner nor an invited user of W.
    let pool = pool().await?;
    let ws = workspace(&pool).await?;
    write(&pool, &ws, "owner", "o", &save("domain", "o", None, "O")).await?;
    let before = read(&pool, &ws, "domain:o").await?;
    let position = head(&pool, &ws).await?;
    let edit = json!({"op":"save_object","kind":"domain","id":"o","revision":before["current_revision"],"parent":null,"data":{"title":"Eve was here","owner":"owner","body":"The rule"},"reason":"Edit"});
    let priority = json!({"op":"set_priority","target":"domain:o","priority":"p0"});
    for command in [edit, priority] {
        let error = write(
            &pool,
            &ws,
            "eve",
            &uuid::Uuid::new_v4().to_string(),
            &command,
        )
        .await
        .expect_err("an uninvited write must be refused");
        ensure!(
            error.to_string().contains("Workspace not found"),
            "{command}: {error}"
        );
    }
    let absent = write(
        &pool,
        "no-such-workspace",
        "eve",
        "x",
        &save("domain", "x", None, "X"),
    )
    .await
    .expect_err("an absent workspace refuses the write");
    ensure!(
        absent.to_string().contains("Workspace not found"),
        "{absent}"
    );
    let after = read(&pool, &ws, "domain:o").await?;
    ensure!(
        after["current_revision"] == before["current_revision"]
            && after["process"]["priority"] == before["process"]["priority"],
        "{after}"
    );
    ensure!(head(&pool, &ws).await? == position, "the journal grew");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_report_upload_updates_the_criterion_and_every_ancestor_counter() -> Result<()> {
    // ac:precomputed-status-reads-ac2: A is the only criterion under R, C and D.
    let pool = pool().await?;
    let ws = workspace(&pool).await?;
    for command in [
        save("domain", "d", None, "D"),
        save("capability", "c", Some("domain:d"), "C"),
        save("requirement", "r", Some("capability:c"), "R"),
        save("ac", "a", Some("requirement:r"), "A"),
        json!({"op":"set_ac_tests","target":"ac:a","tests":["tests/rollup.test.ts > rollup > passes"],"reason":"Bind"}),
    ] {
        write(
            &pool,
            &ws,
            "alex",
            &uuid::Uuid::new_v4().to_string(),
            &command,
        )
        .await?;
    }
    for address in ["requirement:r", "capability:c", "domain:d"] {
        let node = read(&pool, &ws, address).await?;
        ensure!(node["verification_summary"]["passed"] == 0, "{node}");
    }
    let junit = r#"<testsuites><testsuite name="tests/rollup.test.ts"><testsuite name="rollup"><testcase name="passes" /></testsuite></testsuite></testsuites>"#;
    store::upload_report(
        &pool,
        &ws,
        "alex",
        serde_json::from_value(json!({"stamp":{"commit":"a011cf1","order":1,"branch":"master","default_branch":true,"dirty":false,"full":true},"junit":junit}))?,
    )
    .await?;
    // The test status of A is its verification; the read right after the upload shows it.
    let tests = store::criterion_tests(&pool, &ws, "alex", "ac:a").await?;
    ensure!(tests["status"] == "green", "{tests}");
    let a = read(&pool, &ws, "ac:a").await?;
    ensure!(a["signal"]["tone"] == "green", "{a}");
    for address in ["requirement:r", "capability:c", "domain:d"] {
        let node = read(&pool, &ws, address).await?;
        ensure!(node["verification_summary"]["passed"] == 1, "{node}");
    }
    Ok(())
}
