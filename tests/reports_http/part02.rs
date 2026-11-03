// Part 2 of tests/reports_http.rs, included there, so the test names stay unchanged.

// The planner's unstarted list leaves out a criterion that is green by its test reports.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_green_criterion_leaves_the_unstarted_list() -> Result<()> {
    let w = world().await?;
    ready_for_planning(&w).await?;
    // The fixture Task t1 is open work, so retire it: unstarted lists criteria without active work.
    let retire = json!({"op":"retire_task","id":"task:t1","revision":1,"reason":"Fixture work no longer applies"});
    let (code, reply) = post(
        &w,
        &w.owner,
        "/commands",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":retire}),
    )
    .await?;
    ensure!(code == 200, "{code} {reply}");
    let unstarted = || async {
        let reply: Value = w
            .http
            .get(format!("{}/objects?work_filter=unstarted", w.base))
            .bearer_auth(&w.token)
            .send()
            .await?
            .json()
            .await?;
        Ok::<_, anyhow::Error>(
            reply["items"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|item| item["address"] == "ac:key-rights"),
        )
    };
    ensure!(unstarted().await?, "a grey ready criterion is unstarted");
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "the record must make A green"
    );
    ensure!(!unstarted().await?, "a green criterion is not unstarted");
    Ok(())
}

/// A planning workspace on the test server: spec:one with criteria a and b.
/// The token member `agent` is P, and `other` is Q.
struct Planning<'a> {
    w: &'a World,
    pool: deadpool_postgres::Pool,
    ws: String,
}

async fn planning(w: &World) -> Result<Planning<'_>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-planning-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Planning refusals"}),
    )
    .await?;
    for actor in ["agent", "other"] {
        p.write(
            "owner",
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"children","parents":["spec"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["criterion"],"title_preview_length":400},
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
