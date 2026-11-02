// Part 17 of tests/reports_http.rs, included there, so the test names stay unchanged.

impl World {
    /// Accept requirement:key at its current revision; returns the status code and the reply text.
    async fn accept(&self, token: &str, commit: Option<&str>) -> Result<(u16, String)> {
        let read: Value = self
            .http
            .get(format!("{}/objects?address=requirement:key", self.base))
            .bearer_auth(&self.owner)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut command = json!({"op":"accept_commit","target":"requirement:key","revision":read["document"]["revision"]});
        if let Some(commit) = commit {
            command["commit"] = json!(commit);
        }
        let reply = self
            .http
            .post(format!("{}/commands", self.base))
            .bearer_auth(token)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        Ok((reply.status().as_u16(), reply.text().await?))
    }
    async fn acceptance(&self) -> Result<Value> {
        let read: Value = self
            .http
            .get(format!("{}/objects?address=requirement:key", self.base))
            .bearer_auth(&self.owner)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(read["process"]["acceptance"].clone())
    }
}

async fn refused_acceptance(w: &World, commit: &str, error: &str) -> Result<()> {
    let (code, reply) = w.accept(&w.owner, Some(commit)).await?;
    ensure!(code != 200 && reply.contains(error), "{code} {reply}");
    ensure!(w.acceptance().await?.is_null(), "nothing is accepted");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_acceptance_on_a_green_record_names_its_commit_and_actor() -> Result<()> {
    // acceptance-names-subject-ac1, acceptance-shows-actor-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.owner, Some("c1c1c1c")).await?;
    ensure!(code == 200, "{code} {reply}");
    let acceptance = w.acceptance().await?;
    ensure!(
        acceptance["commit"] == "c1c1c1c" && acceptance["actor"] == "owner",
        "{acceptance}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failing_test_on_the_commit_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_revision_without_a_new_run_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac2, new-revision-new-acceptance-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.owner, Some("c1c1c1c")).await?;
    ensure!(code == 200, "{code} {reply}");
    ensure!(
        w.acceptance().await?["commit"] == "c1c1c1c",
        "revision 1 is accepted"
    );
    w.revise_text(1, "Only read and verify, before launch and after")
        .await?;
    ensure!(
        w.acceptance().await?.is_null(),
        "revision 2 is not accepted by the old acceptance"
    );
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_acceptance_without_a_commit_is_refused() -> Result<()> {
    // acceptance-names-subject-ac2
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.owner, None).await?;
    ensure!(
        code != 200 && reply.contains("acceptance needs a commit"),
        "{code} {reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_skipped_test_on_the_commit_refuses_acceptance() -> Result<()> {
    // acceptance-names-subject-ac3
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "skip")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_accept_a_requirement() -> Result<()> {
    // decisions-from-people-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.token, Some("c1c1c1c")).await?;
    ensure!(
        code != 200 && reply.contains("only a person records this decision"),
        "{code} {reply}"
    );
    ensure!(w.acceptance().await?.is_null(), "nothing is accepted");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_without_tests_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1: an untested criterion does not pass by being empty.
    let w = world().await?;
    let command = json!({"op":"save_object","kind":"ac","id":"key-audit","revision":0,"parent":"requirement:key","data":criterion("Every key change is in the audit log"),"reason":"One more criterion"});
    let reply = w
        .http
        .post(format!("{}/commands", w.base))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    ensure!(
        reply.status().is_success(),
        "revise: {}",
        reply.text().await?
    );
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_listed_test_without_a_record_on_the_commit_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1: a listed test that did not run is not a pass.
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}
