#[allow(dead_code)]
mod common;
use anyhow::Result;
use joppa::store;
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn renamed_prefix_preserves_every_old_reference_and_history() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let ws = format!("old-{}", uuid::Uuid::new_v4().simple());
    for (i, value) in common::setup().into_iter().enumerate() {
        store::execute(
            &pool,
            &ws,
            "owner",
            &format!("setup-{i}"),
            &serde_json::from_value(value)?,
        )
        .await?;
    }
    let before = store::read(&pool, &ws, "owner").await?;
    let prefix = format!("renamed-{}", uuid::Uuid::new_v4().simple());
    let command = serde_json::from_value(
        json!({"op":"rename_workspace","title":"Current product","reason":"Owner renamed product","reference_prefix":prefix}),
    )?;
    assert!(
        store::execute(&pool, &ws, "agent", "rename", &command)
            .await
            .is_err()
    );
    store::execute(&pool, &ws, "owner", "rename", &command).await?;
    assert_eq!(
        store::execute(&pool, &ws, "owner", "rename", &command).await?["replayed"],
        true
    );
    let after = store::read(&pool, &ws, "owner").await?;
    let mut expected = before["state"].clone();
    expected["title"] = json!("Current product");
    expected["reference_prefix"] = json!(prefix);
    assert_eq!(after["state"], expected);
    assert_eq!(
        after["history"].as_array().unwrap()[..before["history"].as_array().unwrap().len()],
        *before["history"].as_array().unwrap()
    );
    assert_eq!(after["summary"], before["summary"]);
    for (kind, items) in before["system_ids"].as_object().unwrap() {
        for (local, id) in items.as_object().unwrap() {
            let old = id.as_str().unwrap();
            let new = after["system_ids"][kind][local].as_str().unwrap();
            assert_eq!(new, format!("{prefix}{}", &old[ws.len()..]));
            for reference in [old, new] {
                let found = store::read_item(&pool, reference, "owner").await?;
                assert_eq!(found["selection"]["id"], *local);
                assert_eq!(found["selection"]["kind"], *kind);
                assert!(
                    store::read_item(&pool, reference, "outsider")
                        .await
                        .is_err()
                );
            }
        }
    }
    store::migrate(&pool).await?;
    assert_eq!(store::read(&pool, &ws, "owner").await?, after);
    let added: Value =
        json!({"op":"capability","id":"new","title":"Added after rename","domain":"d"});
    store::execute(&pool, &ws, "owner", "add", &serde_json::from_value(added)?).await?;
    assert!(
        store::read(&pool, &ws, "owner").await?["system_ids"]["capability"]["new"]
            .as_str()
            .unwrap()
            .starts_with(&prefix)
    );
    Ok(())
}
