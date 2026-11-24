// Part 1 of tests/hybrid_search_http.rs: nominal sub-4s concurrent hybrid search check.

// ac:kc74j4qv
// test:6wiw4wjx
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_responsive_provider_within_four_seconds_returns_hybrid_meaning_matches() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = workspace(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    save(&server.root, &ws, "plans", "Renewal of a paid plan", 0).await?;
    settled(&pool, Some(&ws), Duration::from_secs(10)).await?;

    fake.delay_ms.store(250, Ordering::SeqCst);
    let start = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for i in 0..6 {
        let (root, ws) = (server.root.clone(), ws.clone());
        set.spawn(async move {
            if i % 2 == 0 {
                let h =
                    world::http_search(&root, "bot", &ws, &[("q", "extend membership")]).await?;
                ensure!(
                    h["mode"] == "hybrid"
                        && h["semantic_status"] == "ready"
                        && has(&h, "plans"),
                    "{h}"
                );
            } else {
                let m = world::mcp(
                    &root,
                    "bot",
                    "joppa_search",
                    json!({"workspace":ws,"query":"extend membership"}),
                )
                .await?;
                ensure!(has(&m, "plans"), "{m}");
            }
            Ok::<_, anyhow::Error>(())
        });
    }
    while let Some(done) = set.join_next().await {
        done??;
    }
    let elapsed = start.elapsed();
    ensure!(
        elapsed < Duration::from_secs(4),
        "6 concurrent searches against a 250ms provider took {elapsed:?}"
    );
    Ok(())
}
