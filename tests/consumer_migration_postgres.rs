//! A new workspace starts with one consumer per criterion: its first write appends the
//! migrate_consumers event, and its tree has the single required reference on a criterion.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, "owner", &id, &command).await
}

// one-consumer-per-criterion-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_workspace_starts_with_one_consumer_per_criterion() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    ensure!(
        url.rsplit('/')
            .next()
            .is_some_and(|db| db.starts_with("joppa_test")),
        "test database required"
    );
    let pool = store::pool(&url)?;
    store::migrate(&pool).await?;
    let ws = format!("consumer-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "setup",
        &json!({"op":"workspace","title":"One consumer"}),
    )
    .await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    let types = &tree["schema"]["types"];
    ensure!(
        types["ac"]["references"]["consumer"]
            == json!({"target":"consumer","title":"Used by","required":true,"multiple":false,"verification_subject":false}),
        "{types}"
    );
    ensure!(
        types["requirement"].get("references").is_none_or(|r| r
            .as_object()
            .is_some_and(|r| r.values().all(|rule| rule["target"] != "consumer"))),
        "{types}"
    );
    let events: Vec<(String, String)> = pool.get().await?.query("SELECT command->>'op', actor FROM joppa.events WHERE workspace=$1 AND command->>'op'='migrate_consumers'", &[&ws]).await?.iter().map(|r| (r.get(0), r.get(1))).collect();
    ensure!(
        events == vec![("migrate_consumers".to_owned(), "owner".to_owned())],
        "{events:?}"
    );
    let save = |kind: &str, id: &str, parent: Option<&str>, data: Value| json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Build the tree"});
    write(
        &pool,
        &ws,
        save(
            "consumer",
            "buyer",
            None,
            json!({"title":"Buyer","owner":"owner"}),
        ),
    )
    .await?;
    write(
        &pool,
        &ws,
        save(
            "domain",
            "d",
            None,
            json!({"title":"Access","owner":"owner"}),
        ),
    )
    .await?;
    write(
        &pool,
        &ws,
        save(
            "capability",
            "c",
            Some("domain:d"),
            json!({"title":"Keep access","owner":"owner"}),
        ),
    )
    .await?;
    // A requirement names no consumer.
    write(
        &pool,
        &ws,
        save(
            "requirement",
            "r",
            Some("capability:c"),
            json!({"title":"Access persists","owner":"owner","body":"Paid access"}),
        ),
    )
    .await?;
    let criterion = |consumer: Option<Value>| {
        let mut data = json!({"title":"Access persists on iOS","owner":"owner","description":"Given: a buyer.\nWhen: the app restarts.\nThen: access stays.\nEvidence: the read."});
        if let Some(consumer) = consumer {
            data["consumer"] = consumer;
        }
        save("ac", "ios", Some("requirement:r"), data)
    };
    ensure!(
        write(&pool, &ws, criterion(None)).await.is_err(),
        "a criterion without consumer was saved"
    );
    ensure!(
        write(&pool, &ws, criterion(Some(json!(["buyer"]))))
            .await
            .is_err(),
        "a criterion with a list was saved"
    );
    write(&pool, &ws, criterion(Some(json!("buyer")))).await?;
    let read = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("ac:ios"),
            ..Default::default()
        },
    )
    .await?;
    ensure!(read["document"]["data"]["consumer"] == "buyer", "{read}");
    store::check_journal(&pool).await?;
    Ok(())
}
