mod common;

use anyhow::Result;
use common::{World, revision};
use joppa::{commands::Command, store};
use serde_json::{Value, json};

fn external_source() -> Value {
    json!({"op":"link_source","req":"r","revision":1,"id":"external","kind":"tracker","external_id":"issue-42","version":"7","relation":"origin"})
}

#[test]
fn external_tool_source_survives_revision_and_authorized_withdrawal() {
    let mut w = World::new();
    w.ok("agent", external_source());
    let original = w.state.requirements["r"].revisions[0].sources["external"].clone();
    assert_eq!(original.uri, "");
    assert_eq!(original.external_id.as_deref(), Some("issue-42"));
    assert_eq!(original.kind, "tracker");
    assert_eq!(original.version, "7");
    assert_eq!(original.relation, "origin");
    assert_eq!(original.actor, "agent");
    assert_eq!(original.at, "2026-01-01T00:00:00Z");
    assert!(!original.withdrawn);

    let mut withdrawal = json!({"op":"withdraw_source","req":"r","revision":1,"id":"external","reason":"Superseded by a corrected source"});
    w.no("legal", withdrawal.clone());
    w.ok("agent", revision(1));
    w.no("agent", withdrawal.clone());
    withdrawal["revision"] = json!(2);
    w.ok("owner", withdrawal);
    assert_eq!(
        serde_json::to_value(&w.state.requirements["r"].revisions[0].sources["external"]).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    let mut expected = serde_json::to_value(original).unwrap();
    expected["withdrawn"] = json!(true);
    assert_eq!(
        serde_json::to_value(&w.state.requirements["r"].revisions[1].sources["external"]).unwrap(),
        expected
    );
}

#[test]
fn missing_blank_stale_and_duplicate_source_references_do_not_change_state() {
    let mut w = World::new();
    let before = serde_json::to_value(&w.state).unwrap();
    let position = w.position;
    let mut missing = external_source();
    missing.as_object_mut().unwrap().remove("external_id");
    let mut blank = external_source();
    blank["uri"] = json!(" \t ");
    blank["external_id"] = json!(" \n ");
    let mut stale = external_source();
    stale["revision"] = json!(2);
    for command in [missing, blank, stale] {
        w.no("agent", command);
        assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
        assert_eq!(w.position, position);
    }
    w.ok("agent", external_source());
    let before = serde_json::to_value(&w.state).unwrap();
    let position = w.position;
    w.no("agent", external_source());
    assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
    assert_eq!(w.position, position);
}

#[test]
fn uri_and_external_references_remain_compatible_with_event_replay() {
    for (uri, external_id) in [
        ("https://example.test/source", None),
        ("https://example.test/source", Some("issue-42")),
        ("", Some("issue-42")),
    ] {
        let mut w = World::new();
        let before = w.state.clone();
        let position = w.position + 1;
        let mut value = external_source();
        value["uri"] = json!(uri);
        if let Some(id) = external_id {
            value["external_id"] = json!(id);
        } else {
            value.as_object_mut().unwrap().remove("external_id");
        }
        w.ok("agent", value.clone());
        let cmd: Command = serde_json::from_value(value).unwrap();
        let event = serde_json::to_value(cmd).unwrap();
        let replay: Command = serde_json::from_value(event).unwrap();
        let mut recovered = before;
        joppa::commands::apply_replay(
            &mut recovered,
            "agent",
            &replay,
            position,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(recovered).unwrap(),
            serde_json::to_value(&w.state).unwrap()
        );
        let source = &w.state.requirements["r"].revisions[0].sources["external"];
        assert_eq!(source.uri, uri);
        assert_eq!(source.external_id.as_deref(), external_id);
    }
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn external_source_round_trip_preserves_history_and_workspace_isolation() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let workspace = format!("sources-{}", uuid::Uuid::new_v4().simple());
    for (i, value) in common::setup().into_iter().enumerate() {
        store::execute(
            &pool,
            &workspace,
            "owner",
            &format!("setup-{i}"),
            &serde_json::from_value(value)?,
        )
        .await?;
    }
    let foreign = format!("foreign-{}", uuid::Uuid::new_v4().simple());
    store::execute(
        &pool,
        &foreign,
        "other-owner",
        "workspace",
        &serde_json::from_value(json!({"op":"workspace","title":"Other product"}))?,
    )
    .await?;
    let source: Command = serde_json::from_value(external_source())?;
    assert!(
        store::execute(&pool, &foreign, "agent", "foreign-source", &source)
            .await
            .is_err()
    );
    let before = store::read(
        &pool,
        &workspace,
        "owner",
        &store::Scope::from_args(&json!({"req":"r"}))?,
    )
    .await?;
    let mut stale = external_source();
    stale["revision"] = json!(2);
    assert!(
        store::execute(
            &pool,
            &workspace,
            "agent",
            "stale-source",
            &serde_json::from_value(stale)?
        )
        .await
        .is_err()
    );
    assert_eq!(
        store::read(
            &pool,
            &workspace,
            "owner",
            &store::Scope::from_args(&json!({"req":"r"}))?
        )
        .await?,
        before
    );
    store::execute(&pool, &workspace, "agent", "external-source", &source).await?;
    let linked = store::read(
        &pool,
        &workspace,
        "owner",
        &store::Scope::from_args(&json!({"req":"r"}))?,
    )
    .await?;
    let original = linked["requirement"]["revisions"][0]["sources"]["external"].clone();
    assert_eq!(original["uri"], "");
    assert_eq!(original["external_id"], "issue-42");
    assert_eq!(original["actor"], "agent");
    assert!(!original["at"].as_str().unwrap().is_empty());
    store::execute(
        &pool,
        &workspace,
        "agent",
        "next-revision",
        &serde_json::from_value(revision(1))?,
    )
    .await?;
    let withdrawal: Command = serde_json::from_value(
        json!({"op":"withdraw_source","req":"r","revision":2,"id":"external","reason":"Superseded source"}),
    )?;
    assert!(
        store::execute(
            &pool,
            &workspace,
            "legal",
            "invalid-withdrawal",
            &withdrawal
        )
        .await
        .is_err()
    );
    store::execute(&pool, &workspace, "agent", "withdrawal", &withdrawal).await?;
    let historical = store::read(
        &pool,
        &workspace,
        "owner",
        &store::Scope::from_args(&json!({"req":"r","revision":1}))?,
    )
    .await?;
    assert_eq!(
        historical["requirement"]["revisions"][0]["sources"]["external"],
        original
    );
    let current = store::read(
        &pool,
        &workspace,
        "owner",
        &store::Scope::from_args(&json!({"req":"r"}))?,
    )
    .await?;
    let mut expected = original;
    expected["withdrawn"] = json!(true);
    assert_eq!(
        current["requirement"]["revisions"][0]["sources"]["external"],
        expected
    );
    assert!(
        store::read(
            &pool,
            &workspace,
            "other-owner",
            &store::Scope::from_args(&json!({"req":"r"}))?
        )
        .await
        .is_err()
    );
    store::check_journal(&pool).await?;
    Ok(())
}
