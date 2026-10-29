// Part of tests/access/part02.rs: the tail of the test before it, included right after it.

/// A manual deny, a domain move, a rename and a broken configuration each keep the access boundary.
#[allow(clippy::too_many_arguments)]
async fn denial_domain_move_and_rename_keep_the_boundary(
    pool: &deadpool_postgres::Pool,
    path: &std::path::Path,
    id: &str,
    alpha: &str,
    beta: &str,
    alice: &str,
    bob: &str,
    bob_mail: &str,
    mut config: serde_json::Value,
    aliases_path: &std::path::Path,
) -> anyhow::Result<()> {
    assert_eq!(store::list(pool, alice).await?, json!([]));
    assert!(
        store::read(pool, alpha, alice, &store::Scope::full())
            .await
            .is_err()
    );
    assert!(
        store::execute(
            pool,
            alpha,
            alice,
            "denied-write",
            &Command::ConfirmTree(joppa::commands::ConfirmTreeOp {})
        )
        .await
        .is_err()
    );
    assert_eq!(
        store::read(pool, alpha, "admin", &store::Scope::full()).await?["state"]["members"]
            [alice],
        "human"
    );
    // Moving a domain does not leave the old workspace accessible after reenrollment.
    config["domains"]["beta.example"] = json!([alpha]);
    std::fs::write(path, serde_json::to_vec(&config)?)?;
    assert!(
        store::read(pool, beta, bob, &store::Scope::full())
            .await
            .is_err()
    );
    store::enroll(
        pool,
        bob,
        bob,
        bob_mail,
        Some("beta.example"),
        &BTreeSet::from([alpha.to_owned()]),
    )
    .await?;
    assert_eq!(
        store::list(pool, bob).await?,
        json!([{"id":alpha,"title":alpha}])
    );
    assert!(
        store::read(pool, beta, bob, &store::Scope::full())
            .await
            .is_err()
    );
    // Both old policy names and canonical names retain the same access boundary.
    let canonical = format!("canonical-{id}");
    let rename = serde_json::from_value(
        json!({"op":"rename_workspace","title":"Current product","workspace_id":canonical,"reason":"Rename product ID"}),
    )?;
    store::execute(pool, alpha, "admin", "rename", &rename).await?;
    assert_eq!(
        store::list(pool, bob).await?,
        json!([{"id":canonical,"title":"Current product"}])
    );
    config["domains"]["beta.example"] = json!([canonical]);
    std::fs::write(path, serde_json::to_vec(&config)?)?;
    store::enroll(
        pool,
        bob,
        bob,
        bob_mail,
        Some("beta.example"),
        &BTreeSet::from([canonical.clone()]),
    )
    .await?;
    assert_eq!(
        store::read(pool, &canonical, bob, &store::Scope::full()).await?["workspace"],
        canonical
    );
    assert!(
        store::read(pool, beta, bob, &store::Scope::full())
            .await
            .is_err()
    );
    assert!(
        store::read(pool, &canonical, alice, &store::Scope::full())
            .await
            .is_err()
    );
    // Configuration errors fail closed instead of falling back to old membership.
    std::fs::write(path, b"invalid json")?;
    assert!(store::list(pool, bob).await.is_err());
    std::fs::remove_file(path)?;
    assert!(
        store::read(pool, alpha, bob, &store::Scope::full())
            .await
            .is_err()
    );
    std::fs::remove_file(aliases_path)?;
    Ok(())
}
