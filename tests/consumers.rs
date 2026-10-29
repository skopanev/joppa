mod common;

use common::{World, revision};
use serde_json::json;

#[test]
fn requirement_consumers_are_versioned_and_shared_by_all_acs() {
    let mut world = World::new();
    world.legacy("owner", json!({"op":"consumer","id":"support","title":"Support","description":"Uses paid access","owner":"owner"}));
    let mut next = revision(1);
    next["consumers"] = json!(["customer", "support"]);
    world.ok("owner", next);
    assert_eq!(
        world.state.requirements["r"].revisions[1].consumers,
        ["customer", "support"]
    );
    assert!(
        world.state.requirements["r"].revisions[1]
            .acs
            .iter()
            .all(|ac| ac.assessment.is_none())
    );

    let mut corrected = revision(2);
    corrected["consumers"] = json!(["customer"]);
    world.ok("owner", corrected);
    let requirement = &world.state.requirements["r"];
    assert_eq!(requirement.revisions[1].consumers, ["customer", "support"]);
    assert_eq!(requirement.revisions[2].consumers, ["customer"]);
    assert!(
        requirement.revisions[2]
            .acs
            .iter()
            .all(|ac| ac.assessment.is_none())
    );

    let mut unknown = revision(3);
    unknown["consumers"] = json!(["missing"]);
    world.no("owner", unknown);
}

#[test]
fn moving_requirement_and_linking_versioned_source_preserve_history() {
    let mut world = World::new();
    world.legacy(
        "owner",
        json!({"op":"capability","id":"c2","title":"Restore access","domain":"d"}),
    );
    let mut moved = revision(1);
    moved["capability"] = json!("c2");
    moved["consumers"] = json!(["customer"]);
    world.ok("owner", moved);
    assert_eq!(
        world.state.requirements["r"].capability.as_deref(),
        Some("c2")
    );
    assert_eq!(
        world.state.requirements["r"].revisions[0]
            .capability
            .as_deref(),
        Some("c")
    );
    assert_eq!(
        world.state.requirements["r"].revisions[1]
            .capability
            .as_deref(),
        Some("c2")
    );

    world.ok("owner", json!({"op":"link_source","req":"r","revision":2,"id":"policy","kind":"document","uri":"https://example.test/policy","version":"v3","relation":"origin"}));
    assert_eq!(
        world.state.requirements["r"].revisions[1].sources["policy"].version,
        "v3"
    );
    world.no(
        "agent",
        json!({"op":"withdraw_source","req":"r","revision":2,"id":"policy","reason":"Superseded"}),
    );
    world.ok(
        "owner",
        json!({"op":"withdraw_source","req":"r","revision":2,"id":"policy","reason":"Superseded"}),
    );
    assert!(world.state.requirements["r"].revisions[1].sources["policy"].withdrawn);
    let mut next = revision(2);
    next["capability"] = json!("c2");
    world.ok("owner", next);
    assert!(world.state.requirements["r"].revisions[2].sources["policy"].withdrawn);
}

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
    store::execute_input(&pool, &ws, "owner", "source", &json!({"op":"link_source","req":"r","revision":2,"id":"policy","kind":"document","uri":"https://example.test/policy","version":"v3","relation":"origin"})).await?;
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
