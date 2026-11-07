// Part 26 of tests/reports_http.rs, included there, so the test names stay unchanged.

/// The failing check that red sibling B lists in the frozen fixtures.
const REJECTS: &str = "tests/auth/login.test.ts > login > rejects";

/// check-list-frozen-ac3: area G holds R with criterion A and red sibling B; area H is
/// another parent. A lists A and B, B lists a failing check, and one record makes A green.
async fn frozen(w: &World) -> Result<Planning<'_>> {
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
        "criterion":{"title":"Criterion","role":"ac","status":"children","parents":["spec"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["criterion"],"title_preview_length":400},
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
        p.bind(&w.token, ac, &tests).await?;
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
