// Planning claims of two people over HTTP and MCP. P is alex and Q is other.
// Both are human members of the workspace, as planning-claim-ac1 says.

/// A planning workspace with spec:one and its criteria A and B. The owner adds alex (P),
/// other (Q) and worker as human members.
async fn people(w: &World) -> Result<Planning<'_>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-people-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Planning by people"}),
    )
    .await?;
    for actor in ["alex", "other", "worker"] {
        p.write("owner", json!({"op":"member","id":actor,"role":"human"}))
            .await?;
    }
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    p.save(
        "spec:one",
        None,
        json!({"title":"One rule","body":"Implement the agreed rule","owner":"owner"}),
        json!([]),
    )
    .await?;
    for ac in ["criterion:a", "criterion:b"] {
        p.save(
            ac,
            Some("spec:one"),
            json!({"title":ac,"owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    Ok(p)
}

fn claim_command(ac: &str, revision: u64) -> Value {
    json!({"op":"claim_planning","ac":ac,"revision":revision})
}

impl Planning<'_> {
    /// The planning claim of `ac` as the object read returns it.
    async fn planning_claim(&self, ac: &str) -> Result<Value> {
        Ok(self.read(ac).await?["process"]["planning_claim"].clone())
    }
    /// P claims `ac` at its current revision through HTTP or MCP.
    async fn p_claims(&self, mcp: bool, ac: &str) -> Result<Value> {
        let command = claim_command(ac, self.revision(ac).await?);
        let reply = self.send_as(&self.w.alex, mcp, &command).await?;
        reply.map_err(anyhow::Error::msg)
    }
    /// The status and the assignee of T0.
    async fn t0_state(&self) -> Result<Value> {
        let t0 = self.read("work:t0").await?;
        Ok(json!([t0["process"]["status"], t0["process"]["assignee"]]))
    }
}

// planning-claim-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_person_wins_a_criterion_and_claims_another_req_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = people(&w).await?;
    p.ready().await?;
    p.t0().await?;
    let revision = p.revision("work:t0").await?;
    p.write(
        "worker",
        json!({"op":"claim_task","id":"work:t0","revision":revision}),
    )
    .await?;
    let t0 = p.t0_state().await?;
    ensure!(t0 == json!(["in_progress", "worker"]), "{t0}");
    p.prerequisite("Prerequisite scope").await?;

    // P claims A over HTTP. The claim names P and the position of the claim event.
    let a = claim_command("criterion:a", p.revision("criterion:a").await?);
    let reply = p.p_claims(false, "criterion:a").await?;
    let claim = p.planning_claim("criterion:a").await?;
    ensure!(
        claim == json!({"actor":"alex","position":reply["position"]}),
        "{claim} {reply}"
    );
    let client = p.pool.get().await?;
    let events = client
        .query(
            "SELECT position,actor FROM joppa.events WHERE workspace=$1 AND command->>'op'='claim_planning'",
            &[&p.ws],
        )
        .await?;
    ensure!(events.len() == 1, "one claim event");
    ensure!(json!(events[0].get::<_, i64>(0)) == claim["position"]);
    ensure!(events[0].get::<_, String>(1) == "alex");

    // Q loses A over HTTP and MCP, and T0 keeps its status and assignee.
    p.refuse_command_as(
        &w.other,
        "Q claims A of P",
        a,
        "criterion is already claimed for planning",
    )
    .await?;
    ensure!(p.t0_state().await? == t0, "T0 changed");

    // P holds A and claims C of R2 in this workspace over MCP.
    p.p_claims(true, "criterion:c").await?;
    let c_claim = p.planning_claim("criterion:c").await?;
    ensure!(c_claim["actor"] == "alex", "{c_claim}");
    ensure!(p.planning_claim("criterion:a").await? == claim, "P lost A");

    // P claims ready C in another workspace while P holds A here.
    let q = people(&w).await?;
    q.prerequisite("Prerequisite scope").await?;
    q.p_claims(false, "criterion:c").await?;
    ensure!(q.planning_claim("criterion:c").await?["actor"] == "alex");
    ensure!(p.planning_claim("criterion:a").await? == claim, "P lost A");
    ensure!(p.t0_state().await? == t0, "T0 changed");
    store::check_journal(&p.pool).await?;
    Ok(())
}

// planning-claim-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_claim_guard_refuses_a_person_over_http_and_mcp() -> Result<()> {
    const NOT_READY: &str = "criterion is not ready for planning";
    let w = world().await?;
    let ready = json!({"ready":true,"reason":"Known scope"});
    let waiting = json!({"ready":false,"reason":"Unknown input","recipient":"owner"});
    // Each case starts with a new workspace, and all other conditions are valid.
    let refuse = async |p: &Planning<'_>, case: &str, ac: &str, revision: u64, expected: &str| {
        p.refuse_command_as(&w.alex, case, claim_command(ac, revision), expected)
            .await
    };

    let p = people(&w).await?;
    p.t0().await?;
    p.confirm("spec:one", &[("criterion:b", ready.clone())])
        .await?;
    let revision = p.revision("criterion:a").await?;
    refuse(&p, "unassessed A", "criterion:a", revision, NOT_READY).await?;

    let p = people(&w).await?;
    p.t0().await?;
    let assessments = [
        ("criterion:a", waiting.clone()),
        ("criterion:b", ready.clone()),
    ];
    p.confirm("spec:one", &assessments).await?;
    let revision = p.revision("criterion:a").await?;
    refuse(&p, "waiting A", "criterion:a", revision, NOT_READY).await?;

    // A depends on B, and B waits for an input.
    let p = people(&w).await?;
    let data = json!({"title":"criterion:a","owner":"owner","depends_on":["criterion:b"]});
    p.save("criterion:a", Some("spec:one"), data, json!([]))
        .await?;
    p.t0().await?;
    let assessments = [("criterion:a", ready.clone()), ("criterion:b", waiting)];
    p.confirm("spec:one", &assessments).await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["process"]["readiness_blocker"]["criterion"] == "criterion:b",
        "{a}"
    );
    let revision = p.revision("criterion:a").await?;
    refuse(&p, "blocked A", "criterion:a", revision, NOT_READY).await?;

    // A passes by the record of its current test list.
    let p = people(&w).await?;
    p.ready().await?;
    p.t0().await?;
    let tests =
        json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Link the tests"});
    p.send_as(&w.alex, false, &tests)
        .await?
        .map_err(anyhow::Error::msg)?;
    let root = w.base.split("/api/").next().unwrap();
    let base = format!("{root}/api/workspaces/{}", p.ws);
    let passed = outcomes(&[("opens", "pass"), ("refuses", "pass")]);
    record_http::recorded(&w.http, root, &p.ws, &w.alex, "c1c1c1c", passed).await?;
    let status: Value = w
        .http
        .get(format!("{base}/tests?ac=criterion:a"))
        .bearer_auth(&w.alex)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(status["status"] == "green", "the record must make A green: {status}");
    let revision = p.revision("criterion:a").await?;
    let verified = "criterion is already verified";
    refuse(&p, "verified A", "criterion:a", revision, verified).await?;

    let p = people(&w).await?;
    while let n @ ..8 = p.revision("criterion:a").await? {
        let data =
            json!({"title":"criterion:a","owner":"owner","body":format!("Draft {n}")});
        p.save("criterion:a", Some("spec:one"), data, json!([]))
            .await?;
    }
    p.ready().await?;
    p.t0().await?;
    ensure!(p.revision("criterion:a").await? == 8);
    let stale = "stale object revision; read the object again";
    refuse(&p, "revision 8, request 7", "criterion:a", 7, stale).await?;
    ensure!(p.saved_object("criterion:missing").await?.is_null());
    refuse(&p, "unknown A", "criterion:missing", 1, "unknown criterion").await?;
    let revision = p.revision("work:t0").await?;
    let role = "planning requires a criterion";
    refuse(&p, "a Task", "work:t0", revision, role).await?;

    // Control: with all conditions valid, P claims A in the same workspace.
    p.p_claims(false, "criterion:a").await?;
    ensure!(p.planning_claim("criterion:a").await?["actor"] == "alex");
    store::check_journal(&p.pool).await?;
    Ok(())
}
