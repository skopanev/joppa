// Part 26 of tests/reports_http.rs, included there, so the test names stay unchanged.

/// One MCP joppa_batch operation for each criterion.
fn per_criterion(command: impl Fn(&str) -> Value) -> Value {
    json!(["criterion:a", "criterion:b"].map(|ac| json!({"command":command(ac)})))
}

// planning-claim-ac8
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac8_a_planner_claims_and_finishes_two_criteria_of_a_req_in_one_mcp_batch() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    let response = planning_batch_in(&p, claim_batch(&p).await?).await?;
    ensure!(response["result"]["isError"] != true, "{response}");
    for ac in ["criterion:a", "criterion:b"] {
        for (transport, claim) in p.planning_claims(ac).await? {
            ensure!(claim["actor"] == "agent", "{transport} {ac}: {claim}");
        }
    }
    for (id, ac) in [("a1", "criterion:a"), ("b1", "criterion:b")] {
        p.write("agent", json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":ac,"data":{"title":id,"owner":"owner","body":"Implement this criterion","purpose":"implementation"},"reason":"Plan the work"})).await?;
    }
    let finish =
        per_criterion(|ac| json!({"op":"finish_planning","ac":ac,"reason":"Plan complete"}));
    let response = planning_batch_in(&p, finish).await?;
    ensure!(response["result"]["isError"] != true, "{response}");
    for ac in ["criterion:a", "criterion:b"] {
        for (transport, claim) in p.planning_claims(ac).await? {
            ensure!(claim.is_null(), "{transport} {ac}: {claim}");
        }
    }
    for task in ["work:a1", "work:b1"] {
        for (transport, t) in p.entry(&w.token, task).await? {
            ensure!(t["claimable"] == true, "{transport} {task}: {t}");
        }
    }
    store::check_journal(&p.pool).await?;

    // Q holds B: the whole claim batch of P is refused.
    let p = planning(&w).await?;
    p.ready().await?;
    p.claim("other", "criterion:b").await?;
    let claims = claim_batch(&p).await?;
    let before = p.saved().await?;
    let response = planning_batch_in(&p, claims).await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.ends_with("criterion is already claimed for planning")),
        "{response}"
    );
    let after = p.saved().await?;
    ensure!(
        after == before && after["position"] == before["position"],
        "the refused batch changed saved state"
    );
    for (transport, claim) in p.planning_claims("criterion:a").await? {
        ensure!(claim.is_null(), "{transport}: {claim}");
    }
    store::check_journal(&p.pool).await?;
    Ok(())
}

/// claim_planning for A and B at their current revisions.
async fn claim_batch(p: &Planning<'_>) -> Result<Value> {
    let (a, b) = (
        p.revision("criterion:a").await?,
        p.revision("criterion:b").await?,
    );
    Ok(per_criterion(|ac| {
        let revision = if ac == "criterion:a" { a } else { b };
        json!({"op":"claim_planning","ac":ac,"revision":revision})
    }))
}

/// P sends one MCP joppa_batch in the planning workspace.
async fn planning_batch_in(p: &Planning<'_>, operations: Value) -> Result<Value> {
    p.mcp(
        "joppa_batch",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":operations}),
    )
    .await
}

/// The failing check that red sibling B lists in the frozen fixtures.
const REJECTS: &str = "tests/auth/login.test.ts > login > rejects";

/// check-list-frozen-ac3: area G holds R with criterion A and red sibling B; area H is
/// another parent. A lists A and B, B lists a failing check, both are assessed ready,
/// and one record makes A green. With `plan`, the agent claims A for planning
/// and saves draft Task work:t before the run.
async fn frozen(w: &World, plan: bool) -> Result<Planning<'_>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-frozen-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Every change resets"}),
    )
    .await?;
    p.write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure the fixture tree","types":{
        "area":{"title":"Area","role":"group","status":"children","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"]},
        "signal":{"title":"Signal","role":"document","status":"none","root":true,"parents":[],"inbox":true}
    }})).await?;
    for key in ["area:g", "area:h"] {
        p.save(
            key,
            None,
            json!({"title":key,"body":"Group scope","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    p.save(
        "spec:r",
        Some("area:g"),
        json!({"title":"R","body":"Implement the rule","owner":"owner"}),
        json!([]),
    )
    .await?;
    for (ac, tests) in [("criterion:a", vec![A, B]), ("criterion:b", vec![REJECTS])] {
        p.save(
            ac,
            Some("spec:r"),
            json!({"title":ac,"owner":"owner"}),
            json!([]),
        )
        .await?;
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":ac,"tests":tests,"reason":"Bind the tests"}),
        )
        .await?;
    }
    let ready = json!({"ready":true,"reason":"Known scope"});
    p.confirm(
        "spec:r",
        &[("criterion:a", ready.clone()), ("criterion:b", ready)],
    )
    .await?;
    if plan {
        p.claim("agent", "criterion:a").await?;
        p.write("agent", json!({"op":"save_object","kind":"work","id":"t","revision":0,"parent":"criterion:a","data":{"title":"T","owner":"owner","body":"Implement this criterion","purpose":"implementation"},"links":[],"reason":"Plan the criterion"})).await?;
    }
    p.record("c1c1c1c", frozen_run()).await?;
    let (a, b) = (
        p.ac_tests("criterion:a").await?,
        p.ac_tests("criterion:b").await?,
    );
    ensure!(
        a["status"] == "green" && b["status"] == "red",
        "A must start green and B red: {a} {b}"
    );
    Ok(p)
}

/// A record that passes both tests of A and fails the test of B.
fn frozen_run() -> Value {
    outcomes(&[("opens", "pass"), ("refuses", "pass"), ("rejects", "fail")])
}
