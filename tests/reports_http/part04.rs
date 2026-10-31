// Part 4 of tests/reports_http.rs, included there, so the test names stay unchanged.

// Planning claim guards with agent members. The people tests of planning-claim-ac1 are
// in reports_http_parts/planning_people.rs.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_guards_refuse_over_http_and_mcp_without_changing_saved_state() -> Result<()>
{
    let w = world().await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    p.claim("other", "criterion:a").await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "other");
    let revision = p.revision("criterion:a").await?;
    p.refuse(
        "claimed A",
        "criterion:a",
        revision,
        "criterion is already claimed for planning",
    )
    .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    p.prerequisite("Given: a ready criterion of another REQ.\nWhen: it is planned.\nThen: it has a plan.\nEvidence: a Task.")
        .await?;
    p.claim("agent", "criterion:a").await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
    // A planner may hold claims in several REQs.
    p.claim("agent", "criterion:c").await?;
    ensure!(p.read("criterion:c").await?["process"]["planning_claim"]["actor"] == "agent");
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");

    let p = planning(&w).await?;
    p.confirm(
        "spec:one",
        &[("criterion:b", json!({"ready":true,"reason":"Known scope"}))],
    )
    .await?;
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["assessment"]["state"] == "unassessed", "{a}");
    p.refuse(
        "unassessed A",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        "criterion is not ready for planning",
    )
    .await?;

    let p = planning(&w).await?;
    p.confirm(
        "spec:one",
        &[
            (
                "criterion:a",
                json!({"ready":false,"reason":"Unknown input","recipient":"owner"}),
            ),
            ("criterion:b", json!({"ready":true,"reason":"Known scope"})),
        ],
    )
    .await?;
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["assessment"]["state"] == "waiting", "{a}");
    p.refuse(
        "waiting A",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        "criterion is not ready for planning",
    )
    .await?;

    // A blocks on C at C's current revision; C is assessed ready but not green.
    let p = planning(&w).await?;
    let c = p.prerequisite("Prerequisite scope").await?;
    p.depend(c, true).await?;
    p.ready().await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["review_required"] == false
            && a["work_ready"] == false
            && a["process"]["readiness_blocker"]["target"] == "criterion:c"
            && p.revision("criterion:c").await? == c,
        "{a}"
    );
    p.refuse(
        "dependency-blocked A",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        &joppa::dependencies::reason("criterion:c"),
    )
    .await?;

    // dependencies-before-priority: A's non-blocking dependency names an older revision of
    // C. An ordering link of a criterion pins nothing, so it needs no review.
    let p = planning(&w).await?;
    let c = p.prerequisite("Prerequisite scope").await?;
    p.depend(c, false).await?;
    ensure!(p.prerequisite("Sharper prerequisite scope").await? > c);
    p.ready().await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["review_required"] == false
            && a["work_ready"] == true,
        "{a}"
    );
    p.claim("agent", "criterion:a").await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Link the tests"}),
    )
    .await?;
    let root = w.base.split("/api/").next().unwrap();
    let base = format!("{root}/api/workspaces/{}", p.ws);
    let passed = outcomes(&[("opens", "pass"), ("refuses", "pass")]);
    record_http::recorded(&w.http, root, &p.ws, &w.token, "c1c1c1c", passed).await?;
    let status: Value = w
        .http
        .get(format!("{base}/tests?ac=criterion:a"))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(
        status["status"] == "green",
        "the record must make A green: {status}"
    );
    let revision = p.revision("criterion:a").await?;
    p.refuse(
        "verified A",
        "criterion:a",
        revision,
        "criterion is already verified",
    )
    .await?;

    let p = planning(&w).await?;
    while let n @ ..8 = p.revision("criterion:a").await? {
        p.save(
            "criterion:a",
            Some("spec:one"),
            json!({"title":"criterion:a","owner":"owner","body":format!("Draft {n}")}),
            json!([]),
        )
        .await?;
    }
    p.ready().await?;
    p.t0().await?;
    ensure!(p.revision("criterion:a").await? == 8);
    p.refuse(
        "current revision 8, request revision 7",
        "criterion:a",
        7,
        "stale object revision; read the object again",
    )
    .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    ensure!(p.saved_object("criterion:missing").await?.is_null());
    p.refuse("unknown A", "criterion:missing", 1, "unknown criterion")
        .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    let revision = p.revision("work:t0").await?;
    ensure!(revision > 0);
    p.refuse(
        "object without the criterion role",
        "work:t0",
        revision,
        "planning requires a criterion",
    )
    .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}
