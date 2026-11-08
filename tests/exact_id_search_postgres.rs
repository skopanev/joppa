//! A search for an exact ID answers from the database alone: the address, the generated
//! 8-character local ID and the public item ID of an object each give only that object, and
//! no request goes to the embedding provider. A word search still embeds the query.
#[path = "common/pg_plan.rs"]
mod pg_plan;
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::embeddings::Embedder;
use joppa::store;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

// ac:ikc99c7i
// test:3m7nfpj4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_search_for_an_exact_id_sends_no_request_to_the_embedding_provider() -> Result<()> {
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "exact-id").await?;
    // Other objects, so an exact answer that gives one object is a choice.
    pg_plan::many_nodes(&pool, &ws, 5, 0).await?;
    let word = format!("quokka{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let save = json!({"op":"save_object","kind":"domain","revision":0,"parent":null,
        "data":{"title":"Exact target","owner":"owner","body":format!("The body names the {word}.")},
        "reason":"Create"});
    let saved = store::execute_remote_input(&pool, &ws, "owner", "exact-save", &save).await?;
    let address = saved["object"]["address"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let (kind, local) = address.split_once(':').unwrap_or_default();
    ensure!(
        kind == "domain"
            && local.len() == 8
            && local
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
        "{saved}"
    );
    let scope = store::ObjectScope {
        address: Some(&address),
        ..Default::default()
    };
    let read = store::read_objects(&pool, &ws, "bot", &scope).await?;
    let item = read["item_id"].as_str().unwrap_or_default().to_owned();
    ensure!(!item.is_empty() && !item.contains(':'), "{read}");
    let (fake, endpoint) = world::provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    let mut evidence = vec![];
    for query in [address.as_str(), local, item.as_str(), word.as_str()] {
        let reply =
            store::search::find(&pool, "bot", &ws, query, None, 20, Some(&embedder)).await?;
        let calls = fake.calls.load(Ordering::SeqCst);
        evidence.push(json!({"query":query,"provider_requests":calls,"reply":reply}));
        let ids: Vec<(&Value, &Value)> = reply["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| (&i["id"], &i["exact_id"]))
            .collect();
        if query == word {
            ensure!(
                calls == 1,
                "the word search sent {calls} requests: {evidence:?}"
            );
            ensure!(ids.iter().any(|(id, _)| *id == local), "{reply}");
        } else {
            ensure!(calls == 0, "{query} sent {calls} requests: {evidence:?}");
            ensure!(
                reply["semantic_status"] == "exact_match",
                "{query}: {reply}"
            );
            ensure!(
                ids == vec![(&json!(local), &json!(true))],
                "{query}: {reply}"
            );
        }
    }
    println!("{}", json!(evidence));
    Ok(())
}
