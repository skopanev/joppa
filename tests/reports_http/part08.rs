// Part 8 of tests/reports_http.rs, included there, so the test names stay unchanged.

// check-list-frozen-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_group_change_resets_every_criterion_below_it_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = branches(&w).await?;
    let tests = |ac: &str| p.get(format!("/tests?ac={ac}"));
    let object = |address: &str| p.get(format!("/objects?address={address}"));
    let mut before = std::collections::BTreeMap::new();
    for (_, _, ac) in BRANCH_CRITERIA {
        let s = tests(ac).await?;
        ensure!(s["status"] == "green", "{ac} before the change: {s}");
        before.insert(ac, s);
    }
    let g = object("area:g").await?;
    ensure!(
        g["verification_summary"]["passed"].as_u64() > Some(0),
        "{g}"
    );
    let d_readiness = object("criterion:d").await?["process"]["assessment"].clone();
    let h_summary = object("area:h").await?["verification_summary"].clone();

    // Owner S changes only the text of G.
    p.owner_save(
        "area:g",
        None,
        json!({"title":"area:g","body":"Changed group scope","owner":"owner"}),
    )
    .await?;
    for ac in ["criterion:a", "criterion:a2"] {
        let s = tests(ac).await?;
        let old = &before[ac];
        ensure!(
            s["status"] == "grey"
                && addresses(&s) == addresses(old)
                && s["tests_revision"] == old["tests_revision"]
                && s["tests_saved_by"] == old["tests_saved_by"],
            "{ac} after the change: {s}"
        );
        let readiness = object(ac).await?["process"]["assessment"]["state"].clone();
        ensure!(readiness == "unassessed", "{ac} readiness: {readiness}");
    }
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:a"}))
        .await?;
    ensure!(mcp["status"] == "grey", "MCP joppa_tests: {mcp}");
    let d = tests("criterion:d").await?;
    ensure!(d["status"] == "green", "{d}");
    ensure!(
        object("criterion:d").await?["process"]["assessment"] == d_readiness,
        "the readiness of D changed"
    );
    let g = p
        .mcp_read("joppa_objects", json!({"address":"area:g"}))
        .await?;
    ensure!(g["verification_summary"]["passed"] == 0, "{g}");
    ensure!(
        object("area:h").await?["verification_summary"] == h_summary,
        "the summary of H changed"
    );

    // Partial runs after the change combine: the latest result of each test decides.
    p.record("c2c2c2c", outcomes(&[("opens", "pass")])).await?;
    let s = tests("criterion:a").await?;
    ensure!(s["status"] == "grey", "after one record: {s}");
    p.record("c3c3c3c", outcomes(&[("refuses", "pass")]))
        .await?;
    let s = tests("criterion:a").await?;
    ensure!(s["status"] == "green", "after records: {s}");
    p.record(
        "c4c4c4c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let s = tests("criterion:a").await?;
    ensure!(s["status"] == "green", "after a record: {s}");
    ensure!(
        tests("criterion:d").await?["status"] == "green"
            && object("criterion:d").await?["process"]["assessment"] == d_readiness,
        "D changed"
    );

    // An identical revision of G stores no boundary; only a readiness write changes A.
    let revision = p.read("spec:r").await?["process"]["revision"].clone();
    p.write("owner", json!({"op":"assess","req":"spec:r","revision":revision,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    let boundaries = p.boundaries().await?;
    let readiness = object("criterion:a").await?["process"]["assessment"].clone();
    p.owner_save(
        "area:g",
        None,
        json!({"title":"area:g","body":"Changed group scope","owner":"owner"}),
    )
    .await?;
    ensure!(
        p.boundaries().await? == boundaries,
        "an identical revision stored a boundary"
    );
    ensure!(
        tests("criterion:a").await?["status"] == "green"
            && object("criterion:a").await?["process"]["assessment"] == readiness,
        "an identical revision changed A"
    );
    p.write("owner", json!({"op":"assess","req":"spec:r","revision":revision,"ac":"criterion:a","assessment":{"ready":false,"reason":"Which scope?","recipient":"owner"}})).await?;
    let a = object("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "waiting"
            && tests("criterion:a").await?["status"] == "green"
            && p.boundaries().await? == boundaries,
        "{a}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_changed_test_list_makes_a_new_list_revision() -> Result<()> {
    // reduced-coverage-reviewed-ac1: the agent removes one test; the document of the
    // criterion gets a new revision.
    let w = world().await?;
    ensure!(w.status().await?["tests_revision"] == 2, "first list");
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_revision"] == 3
            && s["tests"].as_array().map(Vec::len) == Some(1)
            && s["tests_saved_by"] == "agent",
        "{s}"
    );
    Ok(())
}

const STALE_LIST: &str = "stale test list revision; read the tests again";

/// Save the list of ac:key-rights over HTTP as the agent, with an optional list revision.
async fn save_list(w: &World, tests: &[&str], revision: Option<u32>) -> Result<(u16, Value)> {
    let mut command = json!({"op":"set_ac_tests","target":"ac:key-rights","tests":tests,"reason":"Link the tests"});
    if let Some(revision) = revision {
        command["revision"] = json!(revision);
    }
    post(
        w,
        &w.token,
        "/commands",
        json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
    )
    .await
}

/// The listed test addresses of one test status read.
fn listed(status: &Value) -> Vec<&str> {
    status["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["address"].as_str())
        .collect()
}

async fn journal_count(pool: &deadpool_postgres::Pool, workspace: &str) -> Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1",
            &[&workspace],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stale_test_list_save_is_refused_and_a_current_or_absent_revision_saves() -> Result<()> {
    // checks-listed-first-ac4: criterion C has [A] at tests_revision 3, the revision of
    // its document.
    let w = world().await?;
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(listed(&s) == [A] && s["tests_revision"] == 3, "{s}");
    let (code, reply) = save_list(&w, &[A, B], Some(2)).await?;
    ensure!(
        code == 409 && reply["error"] == STALE_LIST,
        "{code} {reply}"
    );
    let s = w.status().await?;
    ensure!(listed(&s) == [A] && s["tests_revision"] == 3, "{s}");
    let (code, reply) = save_list(&w, &[A, B], Some(3)).await?;
    ensure!(code == 200, "{code} {reply}");
    let s = w.status().await?;
    ensure!(listed(&s) == [A, B] && s["tests_revision"] == 4, "{s}");
    let (code, reply) = save_list(&w, &[B], None).await?;
    ensure!(code == 200, "{code} {reply}");
    let s = w.status().await?;
    ensure!(listed(&s) == [B] && s["tests_revision"] == 5, "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn concurrent_first_test_list_saves_with_revision_zero_accept_exactly_one() -> Result<()> {
    // checks-listed-first-ac4: criterion D has no saved list; agents P and Q save at the same time.
    let w = world().await?;
    let p = planning(&w).await?;
    let root = w.base.split("/api/").next().unwrap();
    let read = || async {
        w.http
            .get(format!(
                "{root}/api/workspaces/{}/tests?ac=criterion:a",
                p.ws
            ))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
    };
    // A criterion without tests has an empty list at its document revision.
    let s = read().await?;
    ensure!(listed(&s).is_empty() && s["tests_revision"] == 1, "{s}");
    let save = |actor: &'static str, tests: Value| {
        p.write(
            actor,
            json!({"op":"set_ac_tests","target":"criterion:a","tests":tests,"revision":1,"reason":"Link the tests"}),
        )
    };
    let (first, second) = tokio::join!(save("agent", json!([A])), save("other", json!([B])));
    let (winner, refusal) = match (first, second) {
        (Ok(_), Err(e)) => (A, e),
        (Err(e), Ok(_)) => (B, e),
        (first, second) => anyhow::bail!("expected one success: {first:?} / {second:?}"),
    };
    ensure!(format!("{refusal:#}") == STALE_LIST, "{refusal:#}");
    let s = read().await?;
    ensure!(listed(&s) == [winner] && s["tests_revision"] == 2, "{s}");
    Ok(())
}
