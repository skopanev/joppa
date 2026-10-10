use joppa::{access::Rules, commands::Command, store};
use serde_json::json;
use std::collections::BTreeSet;

/// The rule file from the environment, or a temporary one for this test binary only.
fn private_file(var: &str, name: &str) -> std::path::PathBuf {
    if let Some(path) = std::env::var_os(var) {
        return path.into();
    }
    let path = std::env::temp_dir().join(format!("joppa-test-{name}-{}.json", std::process::id()));
    // SAFETY: this binary has one test, and it sets the variable before it starts work.
    unsafe { std::env::set_var(var, &path) };
    path
}

#[tokio::test]
#[ignore = "requires isolated test database"]
async fn workspace_enrollment_isolation_revocation_and_identity_binding() -> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let db: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test database required");
    drop(conn);
    store::migrate(&pool).await?;
    let path = private_file("JOPPA_ACCESS_RULES_FILE", "access");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let alpha = format!("alpha-{id}");
    let beta = format!("beta-{id}");
    let alice = format!("alice-{id}");
    let bob = format!("bob-{id}");
    let alice_mail = format!("same-{id}@alpha.example");
    let bob_mail = format!("same-{id}@beta.example");
    let mut config = json!({"domains":{"alpha.example":[alpha],"beta.example":[beta]},"emails":{}});
    std::fs::write(&path, serde_json::to_vec(&config)?)?;
    let aliases_path = private_file("JOPPA_LOGIN_USERS_FILE", "aliases");
    let legacy_actor = format!("legacy-{id}");
    std::fs::write(
        &aliases_path,
        serde_json::to_vec(&json!({"legacy@example.test":legacy_actor}))?,
    )?;
    for workspace in [&alpha, &beta] {
        store::execute(
            &pool,
            workspace,
            "admin",
            "create",
            &Command::Workspace {
                title: workspace.clone(),
            },
        )
        .await?;
    }
    store::execute(
        &pool,
        &alpha,
        "admin",
        "legacy-member",
        &Command::Member {
            id: legacy_actor.clone(),
            role: "human".into(),
        },
    )
    .await?;
    // A previously issued human API key cannot bypass the new access rules.
    assert!(
        store::read(&pool, &alpha, &legacy_actor, &store::Scope::full())
            .await
            .is_err()
    );
    config["emails"]["legacy@example.test"] = json!([alpha]);
    std::fs::write(&path, serde_json::to_vec(&config)?)?;
    assert!(
        store::read(&pool, &alpha, &legacy_actor, &store::Scope::full())
            .await
            .is_ok()
    );
    assert!(
        store::read(&pool, &beta, &legacy_actor, &store::Scope::full())
            .await
            .is_err()
    );
    let rules = Rules::read(&path)?;
    for (actor, email, domain) in [
        (&alice, &alice_mail, "alpha.example"),
        (&bob, &bob_mail, "beta.example"),
    ] {
        store::enroll(
            &pool,
            actor,
            actor,
            email,
            Some(domain),
            &rules.resolve(email, Some(domain)),
        )
        .await?;
    }
    assert_eq!(
        store::list(&pool, &alice).await?,
        json!([{"id":alpha,"title":alpha}])
    );
    assert_eq!(
        store::list(&pool, &bob).await?,
        json!([{"id":beta,"title":beta}])
    );
    for (actor, own, foreign) in [(&alice, &alpha, &beta), (&bob, &beta, &alpha)] {
        assert!(
            store::read(&pool, own, actor, &store::Scope::full())
                .await
                .is_ok()
        );
        let foreign_error = store::read(&pool, foreign, actor, &store::Scope::full())
            .await
            .unwrap_err();
        let missing_error = store::read(&pool, "missing", actor, &store::Scope::full())
            .await
            .unwrap_err();
        assert_eq!(foreign_error.to_string(), missing_error.to_string());
        assert!(foreign_error.is::<store::AccessDenied>());
        for request in ["create", "new-request"] {
            assert!(
                store::execute(
                    &pool,
                    foreign,
                    actor,
                    request,
                    &Command::Member {
                        id: actor.clone(),
                        role: "human".into()
                    }
                )
                .await
                .unwrap_err()
                .is::<store::AccessDenied>()
            );
        }
        assert!(
            store::execute(
                &pool,
                own,
                actor,
                "forge-enrollment",
                &Command::EnrollMember {
                    id: "outsider".into()
                }
            )
            .await
            .is_err()
        );
    }
    // Google accounts cannot claim an existing actor, email, or Google subject.
    assert!(
        store::enroll(
            &pool,
            &alice,
            "different-sub",
            &alice_mail,
            Some("alpha.example"),
            &BTreeSet::from([alpha.clone()])
        )
        .await
        .is_err()
    );
    assert!(
        store::enroll(
            &pool,
            "different-actor",
            &alice,
            &alice_mail,
            Some("alpha.example"),
            &BTreeSet::from([alpha.clone()])
        )
        .await
        .is_err()
    );
    assert!(
        store::enroll(
            &pool,
            &alice,
            &alice,
            &bob_mail,
            Some("beta.example"),
            &BTreeSet::from([beta.clone()])
        )
        .await
        .is_err()
    );
    // A manual deny wins immediately, even though the immutable membership remains.
    config["emails"][&alice_mail] = json!([]);
    std::fs::write(&path, serde_json::to_vec(&config)?)?;
    assert_eq!(store::list(&pool, &alice).await?, json!([]));
    assert!(
        store::read(&pool, &alpha, &alice, &store::Scope::full())
            .await
            .is_err()
    );
    assert!(
        store::execute(
            &pool,
            &alpha,
            &alice,
            "denied-write",
            &Command::ConfirmTree {}
        )
        .await
        .is_err()
    );
    assert_eq!(
        store::read(&pool, &alpha, "admin", &store::Scope::full()).await?["state"]["members"]
            [&alice],
        "human"
    );
    // Moving a domain does not leave the old workspace accessible after reenrollment.
    config["domains"]["beta.example"] = json!([alpha]);
    std::fs::write(&path, serde_json::to_vec(&config)?)?;
    assert!(
        store::read(&pool, &beta, &bob, &store::Scope::full())
            .await
            .is_err()
    );
    store::enroll(
        &pool,
        &bob,
        &bob,
        &bob_mail,
        Some("beta.example"),
        &BTreeSet::from([alpha.clone()]),
    )
    .await?;
    assert_eq!(
        store::list(&pool, &bob).await?,
        json!([{"id":alpha,"title":alpha}])
    );
    assert!(
        store::read(&pool, &beta, &bob, &store::Scope::full())
            .await
            .is_err()
    );
    // Both old policy names and canonical names retain the same access boundary.
    let canonical = format!("canonical-{id}");
    let rename = serde_json::from_value(
        json!({"op":"rename_workspace","title":"Current product","workspace_id":canonical,"reason":"Rename product ID"}),
    )?;
    store::execute(&pool, &alpha, "admin", "rename", &rename).await?;
    assert_eq!(
        store::list(&pool, &bob).await?,
        json!([{"id":canonical,"title":"Current product"}])
    );
    config["domains"]["beta.example"] = json!([canonical]);
    std::fs::write(&path, serde_json::to_vec(&config)?)?;
    store::enroll(
        &pool,
        &bob,
        &bob,
        &bob_mail,
        Some("beta.example"),
        &BTreeSet::from([canonical.clone()]),
    )
    .await?;
    assert_eq!(
        store::read(&pool, &canonical, &bob, &store::Scope::full()).await?["workspace"],
        canonical
    );
    assert!(
        store::read(&pool, &beta, &bob, &store::Scope::full())
            .await
            .is_err()
    );
    assert!(
        store::read(&pool, &canonical, &alice, &store::Scope::full())
            .await
            .is_err()
    );
    // Configuration errors fail closed instead of falling back to old membership.
    std::fs::write(&path, b"invalid json")?;
    assert!(store::list(&pool, &bob).await.is_err());
    std::fs::remove_file(&path)?;
    assert!(
        store::read(&pool, &alpha, &bob, &store::Scope::full())
            .await
            .is_err()
    );
    std::fs::remove_file(aliases_path)?;
    Ok(())
}
