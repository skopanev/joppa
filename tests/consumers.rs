mod common;

use serde_json::json;

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn consumer_refs_and_requirement_revisions_roundtrip_through_postgres() -> anyhow::Result<()>
{
    use joppa::store;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let ws = format!("consumers-{}", uuid::Uuid::new_v4().simple());
    let commands = [
        json!({"op":"workspace","title":"Consumer test"}),
        json!({"op":"domain","id":"d","title":"Access","owner":"owner"}),
        json!({"op":"capability","id":"c","title":"Paid access","domain":"d"}),
        json!({"op":"capability","id":"c2","title":"Restored access","domain":"d"}),
        json!({"op":"consumer","id":"buyer","title":"Buyer","owner":"owner"}),
        json!({"op":"consumer","id":"support","title":"Support","owner":"owner"}),
    ];
    common::seed(&pool, &ws, "owner", commands).await?;
    let first = json!({"op":"revise","req":"r","expected_revision":0,"capability":"c","consumers":["buyer","support"],"title":"Access persists","body":"Paid access","owner":"owner","reason":"Initial scope","acs":[{"id":"ios","text":"Access persists on iOS"}]});
    common::append_legacy(&pool, &ws, "owner", "rev-1", &first).await?;
    // A new criterion takes the first consumer of the revise, so a revise without one is refused.
    let missing = json!({"op":"revise","req":"r","expected_revision":1,"capability":"c","title":"Access persists","body":"Paid access","owner":"owner","reason":"Missing consumer","acs":[{"id":"ios","text":"Access persists on iOS"},{"id":"web","text":"Access persists on the web"}]});
    assert!(
        store::execute_input(&pool, &ws, "owner", "missing", &missing)
            .await
            .is_err()
    );
    let next = json!({"op":"revise","req":"r","expected_revision":1,"capability":"c2","consumers":["buyer"],"title":"Access persists","body":"Paid access","owner":"owner","reason":"Clarified consumer and capability","acs":[{"id":"ios","text":"Access persists on iOS"}]});
    common::append_legacy(&pool, &ws, "owner", "rev-2", &next).await?;
    // link_source is a removed operation: an old journal holds it, and replay reads it.
    common::append_legacy(&pool, &ws, "owner", "source", &json!({"op":"link_source","req":"r","revision":2,"id":"policy","kind":"document","uri":"https://example.test/policy","version":"v3","relation":"origin"})).await?;
    let full = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    assert!(
        full["system_ids"]["consumer"]["buyer"]
            .as_str()
            .unwrap()
            .contains("-U-")
    );
    assert_eq!(
        full["state"]["requirements"]["r"]["revisions"][1]["sources"]["policy"]["version"],
        "v3"
    );
    let old = store::read(
        &pool,
        &ws,
        "owner",
        &store::Scope::from_args(&json!({"req":"r","revision":1}))?,
    )
    .await?;
    assert_eq!(old["requirement"]["capability"], "c");
    assert_eq!(
        old["requirement"]["revisions"][0]["consumers"],
        json!(["buyer", "support"])
    );
    let current = store::read(
        &pool,
        &ws,
        "owner",
        &store::Scope::from_args(&json!({"req":"r"}))?,
    )
    .await?;
    assert_eq!(current["requirement"]["capability"], "c2");
    assert_eq!(
        current["requirement"]["revisions"][0]["consumers"],
        json!(["buyer"])
    );
    Ok(())
}
