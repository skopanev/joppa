mod common;
#[path = "common/resolve_id.rs"]
mod resolve_id;
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
    common::seed(&pool, &ws, "owner", common::setup()).await?;
    let before = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
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
    let after = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    let mut expected = before["state"].clone();
    expected["title"] = json!("Current product");
    expected["reference_prefix"] = json!(prefix);
    // An explicit prefix replaces the prefix that the server derived from the title.
    expected
        .as_object_mut()
        .unwrap()
        .remove("prefix_from_title");
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
            assert_eq!(new, format!("{prefix}{}", &old[old.len() - 13..]));
            for reference in [old, new] {
                assert_eq!(
                    resolve_id::resolve(&pool, "owner", &ws, reference).await?,
                    format!("{kind}:{local}")
                );
                assert!(
                    resolve_id::resolve(&pool, "outsider", &ws, reference)
                        .await
                        .is_err()
                );
            }
        }
    }
    store::migrate(&pool).await?;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
        after
    );
    let added: Value = json!({"op":"save_object","kind":"capability","id":"new","revision":0,"parent":"domain:d","data":{"title":"Added after rename","owner":"owner"},"reason":"Setup"});
    store::execute(&pool, &ws, "owner", "add", &serde_json::from_value(added)?).await?;
    assert!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?["system_ids"]["capability"]
            ["new"]
            .as_str()
            .unwrap()
            .starts_with(&prefix)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn canonical_workspace_rename_preserves_aliases_history_and_isolation() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let old = format!("old-name-{suffix}");
    let new = format!("new-name-{suffix}");
    let owner = format!("owner-{suffix}");
    for value in [
        json!({"op":"workspace","title":"Product"}),
        json!({"op":"save_object","kind":"domain","id":"domain","revision":0,"parent":null,"data":{"title":"Area","owner":owner},"reason":"Setup"}),
    ] {
        store::execute(
            &pool,
            &old,
            &owner,
            &uuid::Uuid::new_v4().to_string(),
            &serde_json::from_value(value)?,
        )
        .await?;
    }
    let before = store::read(&pool, &old, &owner, &store::Scope::full()).await?;
    let rename = serde_json::from_value(
        json!({"op":"rename_workspace","title":"Current product","reason":"Owner changed product ID","workspace_id":new}),
    )?;
    assert!(
        store::execute(&pool, &old, "outsider", "rename", &rename)
            .await
            .is_err()
    );
    store::execute(&pool, &old, &owner, "rename", &rename).await?;
    assert_eq!(
        store::execute(&pool, &new, &owner, "rename", &rename).await?["replayed"],
        true
    );
    let after = store::read(&pool, &new, &owner, &store::Scope::full()).await?;
    assert_eq!(after["workspace"], new);
    assert_eq!(
        store::read(&pool, &old, &owner, &store::Scope::full()).await?,
        after
    );
    assert_eq!(
        store::list(&pool, &owner).await?,
        json!([{"id":new,"title":"Current product"}])
    );
    // A rename without a prefix derives a new prefix from the title; each old ID still reads
    // its object, and each new ID keeps the suffix of the old one.
    for (kind, items) in before["system_ids"].as_object().unwrap() {
        for (local, id) in items.as_object().unwrap() {
            let old_id = id.as_str().unwrap();
            let new_id = after["system_ids"][kind][local].as_str().unwrap();
            assert!(
                new_id.starts_with("current-product-")
                    && new_id.ends_with(&old_id[old_id.len() - 13..])
            );
            for id in [old_id, new_id] {
                assert_eq!(
                    resolve_id::resolve(&pool, &owner, &new, id).await?,
                    format!("{kind}:{local}")
                );
            }
        }
    }
    assert_eq!(
        &after["history"].as_array().unwrap()[..before["history"].as_array().unwrap().len()],
        before["history"].as_array().unwrap()
    );
    let item = before["system_ids"]["domain"]["domain"].as_str().unwrap();
    assert_eq!(
        resolve_id::resolve(&pool, &owner, &new, item).await?,
        "domain:domain"
    );
    for name in [&old, &new] {
        assert!(
            store::read(&pool, name, "outsider", &store::Scope::full())
                .await
                .is_err()
        );
        assert!(
            store::execute(
                &pool,
                name,
                "outsider",
                "takeover",
                &serde_json::from_value(json!({"op":"workspace","title":"Other"}))?
            )
            .await
            .is_err()
        );
    }
    let other = format!("other-{suffix}");
    store::execute(
        &pool,
        &other,
        &owner,
        "create",
        &serde_json::from_value(json!({"op":"workspace","title":"Other"}))?,
    )
    .await?;
    assert!(
        store::execute(&pool, &other, &owner, "claim-name", &rename)
            .await
            .is_err()
    );
    let change = serde_json::from_value(
        json!({"op":"save_object","kind":"capability","id":"ability","revision":0,"parent":"domain:domain","data":{"title":"Ability","owner":owner},"reason":"Setup"}),
    )?;
    let (a, b) = tokio::join!(
        store::execute(&pool, &old, &owner, "concurrent-alias", &change),
        store::execute(&pool, &new, &owner, "concurrent-alias", &change)
    );
    assert!(a.is_ok() && b.is_ok());
    assert_ne!(a?["replayed"], b?["replayed"]);
    store::migrate(&pool).await?;
    assert_eq!(
        store::read(&pool, &old, &owner, &store::Scope::full()).await?["workspace"],
        new
    );
    Ok(())
}
