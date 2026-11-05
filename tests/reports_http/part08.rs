// Part 8 of tests/reports_http.rs, included there, so the test names stay unchanged.

// check-list-frozen-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_group_change_resets_every_criterion_below_it_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = branches(&w).await?;
    let tests = |ac: &'static str| store::test_status(&p.pool, &p.ws, "agent", ac);
    let object = |address: &str| p.get(format!("/objects?address={address}"));
    let (mut before, mut marks) = (
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    );
    for (_, _, ac) in BRANCH_CRITERIA {
        let s = tests(ac).await?;
        ensure!(s["status"] == "green", "{ac} before the change: {s}");
        before.insert(ac, s);
        let mark = object(ac).await?["process"]["assessment"].clone();
        ensure!(mark["state"] == "ready", "{ac} before the change: {mark}");
        marks.insert(ac, mark);
    }
    let tone = |address: &'static str| async move {
        Ok::<_, anyhow::Error>(object(address).await?["signal"]["tone"].clone())
    };
    ensure!(tone("area:g").await? == "green" && tone("area:h").await? == "green");
    let d_readiness = object("criterion:d").await?["process"]["assessment"].clone();

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
        // A change of G keeps the readiness marks below it.
        let readiness = object(ac).await?["process"]["assessment"].clone();
        ensure!(readiness == marks[ac], "{ac} readiness: {readiness}");
    }
    let mcp = p
        .mcp_read("joppa_objects", json!({"address":"criterion:a"}))
        .await?;
    ensure!(mcp["status"]["tone"] == "grey", "MCP joppa_objects: {mcp}");
    let d = tests("criterion:d").await?;
    ensure!(d["status"] == "green", "{d}");
    ensure!(
        object("criterion:d").await?["process"]["assessment"] == d_readiness,
        "the readiness of D changed"
    );
    let g = p
        .mcp_read("joppa_objects", json!({"address":"area:g"}))
        .await?;
    ensure!(g["status"]["tone"] == "grey", "{g}");
    ensure!(tone("area:h").await? == "green", "the tone of H changed");

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
    p.write("owner", json!({"op":"assess","target":"criterion:a","ready":true,"reason":"Known scope"})).await?;
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
    p.write("owner", json!({"op":"assess","target":"criterion:a","ready":false,"reason":"Which scope?"})).await?;
    let a = object("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "waiting"
            && tests("criterion:a").await?["status"] == "green"
            && p.boundaries().await? == boundaries,
        "{a}"
    );
    Ok(())
}
