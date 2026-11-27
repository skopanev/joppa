// Part 3 of tests/p0_feedback_http.rs: ac:wsycpf5i.

// ac:wsycpf5i
// test:9zmegzbl
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p0_5_api_data_relations_and_hash_fields() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_p0_schema(&w).await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,
            "data":{"title":"Domain D","owner":"owner","body":"d1"},"reason":"Create D"}),
    )
    .await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"capability","id":"c","revision":0,"parent":"domain:d",
            "data":{"title":"Cap C","owner":"owner","body":"c1"},
            "relations":{"related":{"edges":[{"to":"domain:d"}]}},
            "reason":"Create C"}),
    )
    .await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"ac","id":"t","revision":0,"parent":"capability:c",
            "data":{"title":"Child T","owner":"owner","body":"t1"},"reason":"Create T"}),
    )
    .await?;

    let cookie = session(&w, "owner").await?;
    let jpr = create_key(&w, &cookie, &w.ws).await?;
    let fields = json!([
        "title",
        "relations",
        "contract_hash",
        "parent_hash",
        "parent_hash_current",
        "parent_changed"
    ]);
    let r1 = read(&w, &jpr, json!({"fields": fields})).await?;
    let find = |reply: &Value, addr: &str| -> Result<Value> {
        reply["items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|i| i["address"] == addr)
            .cloned()
            .with_context(|| format!("missing {addr} in {reply}"))
    };
    let d = find(&r1, "domain:d")?;
    let c = find(&r1, "capability:c")?;
    let t = find(&r1, "ac:t")?;
    ensure!(
        d["relations"] == json!({})
            && is_sha256_hex(&d["contract_hash"])
            && d.get("parent_hash").is_none()
            && d["parent_changed"] == false,
        "root D in /api/data: {d}"
    );
    ensure!(
        c["relations"]["related"]["edges"][0]["to"] == "domain:d"
            && c["contract_hash"] == t["parent_hash_current"]
            && t["parent_hash"] == t["parent_hash_current"]
            && t["parent_changed"] == false,
        "C and T in /api/data: c={c} t={t}"
    );

    ok(
        &w,
        json!({"op":"save_object","kind":"capability","id":"c","revision":1,"data":{"body":"c2"},"reason":"Edit C"}),
    )
    .await?;
    let r2 = read(&w, &jpr, json!({"fields": fields})).await?;
    let t2 = find(&r2, "ac:t")?;
    ensure!(
        t2["parent_changed"] == true && t2["parent_hash"] != t2["parent_hash_current"],
        "T after parent edit in /api/data: {t2}"
    );

    for bad in [
        "relations",
        "contract_hash",
        "parent_hash",
        "parent_hash_current",
        "parent_changed",
    ] {
        let (code, body) = data(&w, Some(&jpr), &json!({"filter": {bad: "x"}})).await?;
        ensure!(
            code == 400
                && body["error"]
                    .as_str()
                    .is_some_and(|e| e.contains(&format!("filter field {bad} is not in the schema"))),
            "filter {bad}: {code} {body}"
        );
    }
    Ok(())
}
