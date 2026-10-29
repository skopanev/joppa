// Part 29 of tests/reports_http.rs, included there, so the test names stay unchanged.

// check-list-frozen-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac1_a_text_change_of_r_greys_its_criteria_and_keeps_the_sibling_requirement_green()
-> Result<()> {
    let w = world().await?;
    let p = Planning {
        w: &w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-frozen-ac1-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"A requirement change resets"}),
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
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]}
    }})).await?;
    // P holds R with criteria A and B, and R2 with criterion C.
    p.save(
        "area:p",
        None,
        json!({"title":"P","body":"Group scope","owner":"owner"}),
        json!([]),
    )
    .await?;
    for spec in ["spec:r", "spec:r2"] {
        p.save(
            spec,
            Some("area:p"),
            json!({"title":spec,"body":"Implement the rule","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    let criteria = [
        ("criterion:a", "spec:r", A),
        ("criterion:b", "spec:r", B),
        ("criterion:c", "spec:r2", REJECTS),
    ];
    for (ac, spec, test) in criteria {
        p.save(
            ac,
            Some(spec),
            json!({"title":ac,"owner":"owner"}),
            json!([]),
        )
        .await?;
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":ac,"tests":[test],"reason":"Bind the tests"}),
        )
        .await?;
    }
    let ready = json!({"ready":true,"reason":"Known scope"});
    p.confirm(
        "spec:r",
        &[
            ("criterion:a", ready.clone()),
            ("criterion:b", ready.clone()),
        ],
    )
    .await?;
    p.confirm("spec:r2", &[("criterion:c", ready)]).await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass"), ("rejects", "pass")]),
    )
    .await?;
    let mut before = std::collections::BTreeMap::new();
    for (ac, _, _) in criteria {
        let s = p.ac_tests(ac).await?;
        let mcp = p.mcp_read("joppa_tests", json!({"ac":ac})).await?;
        let readiness = p.assessment(ac).await?;
        ensure!(
            s["status"] == "green" && mcp["status"] == "green" && readiness["state"] == "ready",
            "{ac} before the change: {s} {mcp} {readiness}"
        );
        before.insert(ac, (s, readiness));
    }

    // Owner S changes only the text of R through the ordinary save.
    p.owner_save(
        "spec:r",
        Some("area:p"),
        json!({"title":"spec:r","body":"Changed rule","owner":"owner"}),
    )
    .await?;
    for ac in ["criterion:a", "criterion:b"] {
        let s = p.ac_tests(ac).await?;
        let mcp = p.mcp_read("joppa_tests", json!({"ac":ac})).await?;
        ensure!(
            s["status"] == "grey"
                && mcp["status"] == "grey"
                && addresses(&s) == addresses(&before[ac].0),
            "{ac} after the change: {s} {mcp}"
        );
        let readiness = p.assessment(ac).await?;
        ensure!(
            readiness["state"] == "unassessed",
            "{ac} readiness: {readiness}"
        );
    }
    let c = p.ac_tests("criterion:c").await?;
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:c"}))
        .await?;
    ensure!(
        c["status"] == "green" && mcp["status"] == "green",
        "C after the change: {c} {mcp}"
    );
    ensure!(
        p.assessment("criterion:c").await? == before["criterion:c"].1,
        "the readiness of C changed"
    );
    let r = p
        .mcp_read("joppa_objects", json!({"address":"spec:r"}))
        .await?;
    ensure!(
        r["verification_summary"]["passed"] == 0,
        "MCP joppa_objects R: {r}"
    );
    Ok(())
}

/// dependencies-before-priority: capability C holds specification B with criterion B1.
/// Specification A sits under `a_parent` with `a_criteria`; capability D exists for AC7.
/// Each criterion is assessed ready and has no test report yet, so each one is grey.
async fn ordering<'a>(w: &'a World, a_parent: &str, a_criteria: &[&str]) -> Result<Planning<'a>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-ordering-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Ordering links"}),
    )
    .await?;
    p.write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure ordered work","types":{
        "cap":{"title":"Capability","role":"group","status":"children","root":true,"parents":[]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["cap"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    for (key, parent) in [
        ("cap:c", None),
        ("cap:d", None),
        ("spec:a", Some(a_parent)),
        ("spec:b", Some("cap:c")),
    ] {
        p.save(
            key,
            parent,
            json!({"title":key,"body":"Deliver the rule","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    for (key, parent) in a_criteria
        .iter()
        .map(|key| (*key, "spec:a"))
        .chain([("criterion:b1", "spec:b")])
    {
        p.save(
            key,
            Some(parent),
            json!({"title":key,"owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    let ready = json!({"ready":true,"reason":"Known scope"});
    let a: Vec<(&str, Value)> = a_criteria.iter().map(|key| (*key, ready.clone())).collect();
    p.confirm("spec:a", &a).await?;
    p.confirm("spec:b", &[("criterion:b1", ready)]).await?;
    Ok(p)
}

/// The status label of an object: HTTP gives `signal`, MCP gives `status`.
fn status_label(t: &Value) -> Option<&str> {
    t["signal"]["label"]
        .as_str()
        .or_else(|| t["status"]["label"].as_str())
}

/// A blocking ordering link without a revision.
fn ordering_link(target: &str) -> Value {
    json!([{"target":target,"relation":"depends_on","blocking":true}])
}
