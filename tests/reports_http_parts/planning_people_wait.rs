// planning-claim-ac1: a person cannot claim a criterion that a blocking link keeps waiting.

// planning-claim-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_person_cannot_claim_a_criterion_that_waits_for_a_blocking_link() -> Result<()> {
    let w = world().await?;
    // A blocks on C at its current revision. C is assessed ready but is not green.
    let p = people(&w).await?;
    let c = p.prerequisite("Prerequisite scope").await?;
    p.depend(c, true).await?;
    p.ready().await?;
    p.t0().await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["review_required"] == false
            && a["work_ready"] == false
            && a["process"]["readiness_blocker"]["target"] == "criterion:c",
        "{a}"
    );
    let revision = p.revision("criterion:a").await?;
    // HTTP and MCP give the same text, and the refusal changes no saved row.
    p.refuse_command_as(
        &w.alex,
        "blocked A",
        claim_command("criterion:a", revision),
        "waits for criterion:c, which is not green",
    )
    .await?;
    ensure!(p.planning_claim("criterion:a").await?.is_null());
    Ok(())
}
