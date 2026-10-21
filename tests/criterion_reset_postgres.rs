//! A change of a group, a test-list save and a spec archive reset the criteria below
//! them in the store: in the same batch, after journal replay, and not at all when the
//! command fails. An identical repeat of a record after the change is a new
//! receipt that counts after it.
use anyhow::{Result, ensure};
use joppa::store::{self, BatchOperation};
use serde_json::{Value, json};

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
    store::criterion_tests(pool, ws, "owner", ac).await
}

fn save_g(revision: &Value, body: &str) -> Value {
    json!({"op":"save_object","kind":"area","id":"g","revision":revision,"parent":null,"data":{"title":"G","body":body,"owner":"owner"},"reason":"Change the group"})
}

/// G holds spec R with criterion A; sibling group H holds spec RH with criterion D.
/// Both list A and B, one record passes both, and both are assessed ready.
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
        "area":{"title":"Area","role":"group","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]}
    }})).await?;
    for (area, spec, criterion) in [("g", "r", "a"), ("h", "rh", "d")] {
        write(pool, &ws, "owner", json!({"op":"save_object","kind":"area","id":area,"revision":0,"parent":null,"data":{"title":area.to_uppercase(),"body":"Group scope","owner":"owner"},"reason":"Create the group"})).await?;
        write(pool, &ws, "owner", json!({"op":"save_object","kind":"spec","id":spec,"revision":0,"parent":format!("area:{area}"),"data":{"title":spec,"body":"Implement the rule","owner":"owner"},"reason":"Create the spec"})).await?;
        write(pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":criterion,"revision":0,"parent":format!("spec:{spec}"),"data":{"title":criterion,"owner":"owner"},"reason":"Create the criterion"})).await?;
        write(pool, &ws, "planner", json!({"op":"set_ac_tests","target":format!("criterion:{criterion}"),"tests":[A,B],"reason":"Bind the tests"})).await?;
    }
    upload_c1(pool, &ws).await?;
    for (spec, criterion) in [("spec:r", "criterion:a"), ("spec:rh", "criterion:d")] {
        let revision = read(pool, &ws, spec).await?["process"]["revision"].clone();
        write(
            pool,
            &ws,
            "owner",
            json!({"op":"confirm","req":spec,"revision":revision}),
        )
        .await?;
        write(pool, &ws, "planner", json!({"op":"assess","req":spec,"revision":revision,"ac":criterion,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
    }
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

/// The owner changes the text of criterion A, so A is grey with a draft list.
async fn change_a(pool: &deadpool_postgres::Pool, ws: &str) -> Result<()> {
    let a = read(pool, ws, "criterion:a").await?["current_revision"].clone();
    write(pool, ws, "owner", json!({"op":"save_object","kind":"criterion","id":"a","revision":a,"parent":"spec:r","data":{"title":"a changed","owner":"owner"},"reason":"Sharper wording"})).await?;
    let s = status(pool, ws, "criterion:a").await?;
    ensure!(
        s["status"] == "grey" && s["tests_draft"] == true,
        "after the change: {s}"
    );
    Ok(())
}

/// The ID of the C1 report and the number of its repeat receipts.
async fn c1_receipts(pool: &deadpool_postgres::Pool, ws: &str) -> Result<(i64, i64)> {
    let row = pool.get().await?.query_one("SELECT r.id,(SELECT count(*) FROM joppa.test_report_receipts c WHERE c.report_id=r.id) FROM joppa.test_reports r WHERE r.workspace=$1 AND r.commit_sha='c1c1c1c'", &[&ws]).await?;
    Ok((row.get(0), row.get(1)))
}

/// The state that replay must reproduce: statuses, readiness and stored boundaries.
async fn snapshot(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    let mut snapshot = json!({});
    for ac in ["criterion:a", "criterion:d"] {
        snapshot[ac] = json!({"status":status(pool, ws, ac).await?,"assessment":read(pool, ws, ac).await?["process"]["assessment"]});
    }
    let boundaries: String = pool.get().await?.query_one("SELECT COALESCE(jsonb_object_agg(key,value),'{}')::text FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries'", &[&ws]).await?.get(0);
    snapshot["boundaries"] = serde_json::from_str(&boundaries)?;
    Ok(snapshot)
}

// check-list-frozen-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_group_change_reaches_the_batch_planning_read_and_survives_replay() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    let a = read(&pool, &ws, "criterion:a").await?;
    let req = read(&pool, &ws, "spec:r").await?["process"]["revision"].clone();
    let claim = json!({"op":"claim_planning","ac":"criterion:a","revision":a["current_revision"]});
    let refused = store::execute_remote_batch(
        &pool,
        &ws,
        "planner",
        &uuid::Uuid::new_v4().to_string(),
        &[BatchOperation {
            reference: None,
            command: claim.clone(),
        }],
    )
    .await;
    ensure!(
        refused.is_err_and(|e| format!("{e:#}").contains("criterion is already verified")),
        "a green criterion must refuse planning"
    );
    // The list save of D stored its boundary; the change of G must keep it.
    let d = snapshot(&pool, &ws).await?["boundaries"]["criterion:d"].clone();
    let g = read(&pool, &ws, "area:g").await?["current_revision"].clone();
    let operations: Vec<BatchOperation> = [
        save_g(&g, "Changed group scope"),
        json!({"op":"assess","req":"spec:r","revision":req,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}}),
        claim,
    ]
    .into_iter()
    .map(|command| BatchOperation { reference: None, command })
    .collect();
    // The planning read in the same batch sees the change of G, so A is grey there.
    store::execute_remote_batch(
        &pool,
        &ws,
        "planner",
        &uuid::Uuid::new_v4().to_string(),
        &operations,
    )
    .await?;
    ensure!(
        read(&pool, &ws, "criterion:a").await?["process"]["planning_claim"]["actor"] == "planner",
        "the claim must follow the grey planning read"
    );
    let live = snapshot(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "grey"
            && live["criterion:d"]["status"]["status"] == "green"
            && live["boundaries"].get("criterion:a").is_some()
            && live["boundaries"]["criterion:d"] == d,
        "{live}"
    );
    // Replay the journal into a new projection of the workspace.
    pool.get()
        .await?
        .execute("DELETE FROM joppa.state_entries WHERE workspace=$1", &[&ws])
        .await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::migrate(&pool).await?;
    let replayed = snapshot(&pool, &ws).await?;
    ensure!(replayed == live, "replay: {replayed}\nlive: {live}");
    Ok(())
}

// check-list-frozen-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failed_boundary_write_rejects_the_group_change() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    let events = |pool: deadpool_postgres::Pool, ws: String| async move {
        Ok::<i64, anyhow::Error>(
            pool.get()
                .await?
                .query_one(
                    "SELECT count(*) FROM joppa.events WHERE workspace=$1",
                    &[&ws],
                )
                .await?
                .get(0),
        )
    };
    let before = snapshot(&pool, &ws).await?;
    let count = events(pool.clone(), ws.clone()).await?;
    let conn = pool.get().await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_reset_fails() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'boundary write failed'; END $$;").await?;
    conn.batch_execute(&format!("CREATE TRIGGER test_reset_fails BEFORE INSERT OR UPDATE ON joppa.state_entries FOR EACH ROW WHEN (NEW.workspace='{ws}' AND NEW.section='criterion_boundaries') EXECUTE FUNCTION joppa.test_reset_fails()")).await?;
    let g = read(&pool, &ws, "area:g").await?["current_revision"].clone();
    let result = write(&pool, &ws, "owner", save_g(&g, "Changed group scope")).await;
    conn.batch_execute("DROP TRIGGER test_reset_fails ON joppa.state_entries")
        .await?;
    ensure!(
        result.is_err_and(|e| format!("{e:#}").contains("boundary write failed")),
        "the change must be rejected"
    );
    ensure!(
        events(pool.clone(), ws.clone()).await? == count,
        "the journal has a new entry"
    );
    let after = snapshot(&pool, &ws).await?;
    ensure!(after == before, "after: {after}\nbefore: {before}");
    ensure!(
        after["criterion:a"]["status"]["status"] == "green"
            && after["criterion:a"]["assessment"]["state"] == "ready",
        "{after}"
    );
    Ok(())
}

/// Replay the journal into a new projection of the workspace and return its snapshot.
async fn replay(pool: &deadpool_postgres::Pool, ws: &str) -> Result<Value> {
    pool.get()
        .await?
        .execute("DELETE FROM joppa.state_entries WHERE workspace=$1", &[&ws])
        .await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::migrate(pool).await?;
    snapshot(pool, ws).await
}

/// The number of journal events of the workspace.
async fn events(pool: &deadpool_postgres::Pool, ws: &str) -> Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0))
}

/// A record that passes A and B and fails the check of red criterion B.
async fn upload_full(pool: &deadpool_postgres::Pool, ws: &str, commit: &str) -> Result<Value> {
    let rejects = "tests/auth/login.test.ts > login > rejects";
    store::record_results(
        pool,
        ws,
        "planner",
        &json!({"commit":commit,"results":[{"test":A,"status":"pass"},{"test":B,"status":"pass"},{"test":rejects,"status":"fail","message":"boom"}]}),
    )
    .await
}

fn resave_a() -> Value {
    json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Save the list again"})
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_save_and_a_spec_archive_survive_replay() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    // Red criterion B is a sibling of A under R.
    write(&pool, &ws, "owner", json!({"op":"save_object","kind":"criterion","id":"b","revision":0,"parent":"spec:r","data":{"title":"b","owner":"owner"},"reason":"Create the criterion"})).await?;
    write(&pool, &ws, "planner", json!({"op":"set_ac_tests","target":"criterion:b","tests":["tests/auth/login.test.ts > login > rejects"],"reason":"Bind the tests"})).await?;
    upload_full(&pool, &ws, "c2c2c2c").await?;
    let b = status(&pool, &ws, "criterion:b").await?;
    ensure!(b["status"] == "red", "{b}");
    // Fixture 5: the planner saves the identical list of A.
    write(&pool, &ws, "planner", resave_a()).await?;
    let live = snapshot(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "grey"
            && live["criterion:a"]["assessment"]["state"] == "unassessed"
            && live["criterion:d"]["status"]["status"] == "green",
        "{live}"
    );
    ensure!(
        replay(&pool, &ws).await? == live,
        "replay after the list save"
    );
    // A turns green and ready again; fixture 6: the owner archives B.
    let req = read(&pool, &ws, "spec:r").await?["process"]["revision"].clone();
    write(&pool, &ws, "planner", json!({"op":"assess","req":"spec:r","revision":req,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    upload_full(&pool, &ws, "c3c3c3c").await?;
    ensure!(
        status(&pool, &ws, "criterion:a").await?["status"] == "green",
        "A must be green before the archive"
    );
    let b = read(&pool, &ws, "criterion:b").await?["current_revision"].clone();
    write(&pool, &ws, "owner", json!({"op":"archive_object","target":"criterion:b","revision":b,"reason":"Drop the red criterion"})).await?;
    let live = snapshot(&pool, &ws).await?;
    ensure!(
        live["criterion:a"]["status"]["status"] == "grey"
            && live["criterion:a"]["assessment"]["state"] == "unassessed"
            && live["criterion:d"]["status"]["status"] == "green",
        "{live}"
    );
    let replayed = replay(&pool, &ws).await?;
    ensure!(replayed == live, "replay: {replayed}\nlive: {live}");
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failed_reset_rejects_the_list_save() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    let before = snapshot(&pool, &ws).await?;
    let count = events(&pool, &ws).await?;
    let conn = pool.get().await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_list_reset_fails() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'list reset failed'; END $$;").await?;
    conn.batch_execute(&format!("CREATE TRIGGER test_list_reset_fails BEFORE INSERT OR UPDATE ON joppa.state_entries FOR EACH ROW WHEN (NEW.workspace='{ws}' AND NEW.section='criterion_boundaries') EXECUTE FUNCTION joppa.test_list_reset_fails()")).await?;
    let result = write(&pool, &ws, "planner", resave_a()).await;
    conn.batch_execute("DROP TRIGGER test_list_reset_fails ON joppa.state_entries")
        .await?;
    ensure!(
        result.is_err_and(|e| format!("{e:#}").contains("list reset failed")),
        "the list save must be rejected"
    );
    ensure!(
        events(&pool, &ws).await? == count,
        "the journal has a new entry"
    );
    let after = snapshot(&pool, &ws).await?;
    ensure!(after == before, "after: {after}\nbefore: {before}");
    ensure!(
        after["criterion:a"]["status"]["status"] == "green"
            && after["criterion:a"]["assessment"]["state"] == "ready",
        "{after}"
    );
    Ok(())
}

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failed_receipt_write_rejects_the_identical_upload() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    change_a(&pool, &ws).await?;
    let (id, receipts) = c1_receipts(&pool, &ws).await?;
    let conn = pool.get().await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_receipt_fails() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'receipt write failed'; END $$;").await?;
    conn.batch_execute(&format!("CREATE TRIGGER test_receipt_fails BEFORE INSERT ON joppa.test_report_receipts FOR EACH ROW WHEN (NEW.report_id={id}) EXECUTE FUNCTION joppa.test_receipt_fails()")).await?;
    let result = upload_c1(&pool, &ws).await;
    conn.batch_execute("DROP TRIGGER test_receipt_fails ON joppa.test_report_receipts")
        .await?;
    ensure!(
        result.is_err_and(|e| format!("{e:#}").contains("receipt write failed")),
        "the upload must fail"
    );
    ensure!(
        c1_receipts(&pool, &ws).await? == (id, receipts),
        "the report has a new receipt"
    );
    let s = status(&pool, &ws, "criterion:a").await?;
    ensure!(s["status"] == "grey", "after the failed upload: {s}");
    Ok(())
}

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_planning_read_sees_the_receipt_of_an_identical_upload() -> Result<()> {
    let pool = pool().await?;
    let ws = branches(&pool).await?;
    change_a(&pool, &ws).await?;
    // The change cleared the readiness of A; the planner assesses it again.
    let req = read(&pool, &ws, "spec:r").await?["process"]["revision"].clone();
    write(&pool, &ws, "planner", json!({"op":"assess","req":"spec:r","revision":req,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    let claim = |pool: deadpool_postgres::Pool, ws: String| async move {
        let a = read(&pool, &ws, "criterion:a").await?["current_revision"].clone();
        write(
            &pool,
            &ws,
            "planner",
            json!({"op":"claim_planning","ac":"criterion:a","revision":a}),
        )
        .await
    };
    // The planning read gives grey: the claim passes, then the planner releases it.
    claim(pool.clone(), ws.clone()).await?;
    write(
        &pool,
        &ws,
        "planner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Read the status again"}),
    )
    .await?;
    let (id, receipts) = c1_receipts(&pool, &ws).await?;
    let reply = upload_c1(&pool, &ws).await?;
    ensure!(
        reply["record"] == id && reply["replayed"] == true,
        "{reply}"
    );
    ensure!(
        c1_receipts(&pool, &ws).await? == (id, receipts + 1),
        "the repeat must add one receipt"
    );
    let s = status(&pool, &ws, "criterion:a").await?;
    ensure!(s["status"] == "green", "after the record: {s}");
    // The planning read gives green as well: the claim is refused.
    let refused = claim(pool.clone(), ws.clone()).await;
    ensure!(
        refused.is_err_and(|e| format!("{e:#}").contains("criterion is already verified")),
        "a green criterion must refuse planning"
    );
    Ok(())
}
