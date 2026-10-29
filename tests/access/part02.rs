// Part 2 of tests/access.rs, included there, so the test names stay unchanged.

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
    denial_domain_move_and_rename_keep_the_boundary(&pool, &path, &id, &alpha, &beta, &alice, &bob, &bob_mail, config, &aliases_path).await
}
