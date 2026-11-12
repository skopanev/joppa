// Part 31 of tests/reports_http.rs, included there, so the test names stay unchanged.

// dependencies-before-priority-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_a_dependency_cycle_through_the_tree_is_refused_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    p.link("spec:b", ordering_link("spec:a")).await?;
    let save = |key: &str, object: &Value, relations: Value| {
        let (kind, id) = key.split_once(':').unwrap();
        json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],"parent":object["document"]["parent"],
            "data":object["document"]["data"],"relations":relations,"reason":"Order the work"})
    };
    // Each refusal leaves the committed rows and the journal position unchanged.
    for (case, source, target) in [
        ("fixture 1: A1 depends on B1", "criterion:a1", "criterion:b1"),
        ("fixture 2: A depends on B", "spec:a", "spec:b"),
    ] {
        let object = p.read(source).await?;
        p.refuse_command_as(
            &w.owner,
            case,
            save(source, &object, ordering_link(target)),
            "dependency cycle",
        )
        .await?;
        ensure!(
            depends_on_targets(&p.object_both(source).await?).is_empty(),
            "{case}"
        );
    }
    // Controls: a related edge, a depends_on edge to an unrelated requirement and the removal
    // of the edge from B to A make no cycle.
    p.save(
        "spec:d",
        Some("cap:c"),
        json!({"title":"spec:d","body":"Deliver another rule","owner":"owner"}),
        json!({}),
    )
    .await?;
    p.link("spec:a", json!({"related":{"edges":[{"to":"spec:b"}]}}))
        .await?;
    p.link("criterion:b1", ordering_link("spec:d")).await?;
    p.link("spec:b", json!({"depends_on":null})).await?;
    ensure!(depends_on_targets(&p.read("criterion:b1").await?) == ["spec:d"]);
    ensure!(depends_on_targets(&p.read("spec:b").await?).is_empty());
    store::check_journal(&p.pool).await?;
    Ok(())
}
