//! requirement:embedding-model, the provider contract without a database: the fixed model,
//! profile and endpoint, the request body, the batch size, the HTTPS rule and the reduction
//! of each 4096-value reply to a unit vector of its first 1024 values.
use anyhow::{Result, ensure};
use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use joppa::embeddings::{DIMS, ENDPOINT, Embedder, MODEL, PROFILE};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// The authorization header and the JSON body of one request.
type Request = (Option<String>, Value);

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<Request>>>);

/// Each text gets the values 1, 2, 3 ... 4096, so the expected prefix is known.
async fn embed(
    State(seen): State<Seen>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    seen.0.lock().unwrap().push((auth, body.clone()));
    let count = body["input"].as_array().map_or(0, Vec::len);
    let vector: Vec<f32> = (1..=4096).map(|v| v as f32).collect();
    let data: Vec<Value> = (0..count)
        .rev()
        .map(|index| json!({"index":index,"embedding":vector}))
        .collect();
    Json(json!({"data":data}))
}

async fn provider() -> Result<(Seen, String)> {
    let seen = Seen::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new()
        .route("/embeddings", post(embed))
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Ok((seen, format!("http://{addr}/embeddings")))
}

// embedding-model-fixed
#[test]
fn the_model_profile_dimensions_and_endpoint_are_fixed() {
    assert_eq!(MODEL, "Qwen/Qwen3-Embedding-8B");
    assert_eq!(PROFILE, "Qwen/Qwen3-Embedding-8B:4096:MRL1024:v1");
    assert_eq!(DIMS, 1024);
    assert_eq!(ENDPOINT, "https://api.deepinfra.com/v1/openai/embeddings");
}

// embedding-request-shape
#[tokio::test]
async fn the_provider_request_names_the_model_and_asks_for_floats() -> Result<()> {
    let (seen, endpoint) = provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    let vectors = embedder.embed(&["first".into(), "second".into()]).await?;
    let requests = seen.0.lock().unwrap().clone();
    ensure!(requests.len() == 1, "{requests:?}");
    let (auth, body) = &requests[0];
    ensure!(auth.as_deref() == Some("Bearer test-only-key"), "{auth:?}");
    ensure!(
        *body
            == json!({"model":"Qwen/Qwen3-Embedding-8B","input":["first","second"],"encoding_format":"float"}),
        "{body}"
    );
    ensure!(vectors.len() == 2, "one vector for each text");
    Ok(())
}

// embedding-normalized-prefix
#[tokio::test]
async fn a_vector_keeps_the_first_1024_of_4096_values_at_unit_length() -> Result<()> {
    let (_seen, endpoint) = provider().await?;
    let vectors = Embedder::new("test-only-key".into(), endpoint)?
        .embed(&["text".into()])
        .await?;
    let vector = &vectors[0];
    ensure!(vector.len() == 1024, "{} values", vector.len());
    let norm: f64 = (1..=1024)
        .map(|v: i32| f64::from(v).powi(2))
        .sum::<f64>()
        .sqrt();
    for (i, value) in vector.iter().enumerate() {
        let expected = (i + 1) as f64 / norm;
        ensure!(
            (f64::from(*value) - expected).abs() < 1e-6,
            "value {i}: {value}"
        );
    }
    let length: f64 = vector
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        .sqrt();
    ensure!((length - 1.0).abs() < 1e-5, "length {length}");
    Ok(())
}

// embedding-batch-size
#[tokio::test]
async fn a_request_holds_1_to_16_texts() -> Result<()> {
    let (seen, endpoint) = provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    let texts = |n: usize| (0..n).map(|i| format!("text {i}")).collect::<Vec<_>>();
    ensure!(embedder.embed(&[]).await.is_err(), "an empty batch passed");
    ensure!(embedder.embed(&texts(17)).await.is_err(), "17 texts passed");
    ensure!(
        seen.0.lock().unwrap().is_empty(),
        "a refused batch reached the provider"
    );
    ensure!(
        embedder.embed(&texts(16)).await?.len() == 16,
        "16 texts failed"
    );
    Ok(())
}

// embedding-endpoint-https
#[test]
fn the_endpoint_uses_https_and_only_loopback_may_use_http() {
    let new = |url: &str| Embedder::new("test-only-key".into(), url.into());
    assert!(new(ENDPOINT).is_ok());
    assert!(new("https://embeddings.example.test/v1").is_ok());
    assert!(new("http://127.0.0.1:9/embeddings").is_ok());
    assert!(new("http://localhost:9/embeddings").is_ok());
    assert!(new("http://[::1]:9/embeddings").is_ok());
    assert!(new("http://embeddings.example.test/v1").is_err());
    assert!(new("ftp://127.0.0.1/embeddings").is_err());
}
