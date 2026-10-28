//! DeepInfra produces vectors; PostgreSQL stores them. No local inference.
use crate::embeddings_limit::{InputFault, Limits, ProviderStatus, is_input_fault};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::time::Duration;

pub const MODEL: &str = "Qwen/Qwen3-Embedding-8B";
pub const PROFILE: &str = "Qwen/Qwen3-Embedding-8B:4096:MRL1024:v1";
pub const DIMS: usize = 1024;

#[derive(Clone)]
pub struct Embedder {
    http: reqwest::Client,
    key: String,
    endpoint: String,
    limits: Limits,
}
#[derive(Deserialize)]
struct Response {
    data: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    index: usize,
    embedding: Vec<f32>,
}

impl Embedder {
    /// Search is always hybrid, so the server needs the provider key to start.
    /// JOPPA_EMBEDDING_URL replaces the provider endpoint for local tests only.
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("DEEPINFRA_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .context(
                "set DEEPINFRA_API_KEY: search is always hybrid and needs the embedding provider",
            )?;
        let endpoint = std::env::var("JOPPA_EMBEDDING_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "https://api.deepinfra.com/v1/openai/embeddings".into());
        Self::new(key, endpoint)
    }
    // Explicit endpoint injection is for local provider-contract tests only.
    pub fn new(key: String, endpoint: String) -> Result<Self> {
        let url = reqwest::Url::parse(&endpoint)?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))),
            "embedding endpoint must use HTTPS"
        );
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            key,
            endpoint,
            limits: Limits::default(),
        })
    }
    pub async fn embed(&self, input: &[String]) -> Result<Vec<Vec<f32>>> {
        ensure!(
            !input.is_empty() && input.len() <= 16,
            "embedding batch must contain 1–16 documents"
        );
        // At most 4 open requests and 200 request starts in 60 seconds, across the
        // indexer and the queries.
        let _slot = self.limits.admit().await?;
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.key)
            .json(&serde_json::json!({"model":MODEL,"input":input,"encoding_format":"float"}))
            .send()
            .await
            .context("embedding provider unavailable")?;
        let status = response.status().as_u16();
        if is_input_fault(status) {
            return Err(InputFault(status).into());
        }
        if !response.status().is_success() {
            return Err(ProviderStatus(status).into());
        }
        let response: Response = response
            .json()
            .await
            .context("invalid embedding response")?;
        ensure!(
            response.data.len() == input.len(),
            "embedding count mismatch"
        );
        let mut output = vec![None; input.len()];
        for entry in response.data {
            ensure!(
                entry.index < input.len() && output[entry.index].is_none(),
                "invalid embedding index"
            );
            output[entry.index] = Some(normalize(entry.embedding)?);
        }
        output
            .into_iter()
            .map(|v| v.context("missing embedding"))
            .collect()
    }
}
fn normalize(mut embedding: Vec<f32>) -> Result<Vec<f32>> {
    ensure!(
        embedding.len() == 4096 && embedding.iter().all(|v| v.is_finite()),
        "invalid embedding dimensions or values"
    );
    // Qwen3 supports Matryoshka dimensions. Normalize the retained prefix.
    embedding.truncate(DIMS);
    let norm = embedding
        .iter()
        .map(|&v| f64::from(v).powi(2))
        .sum::<f64>()
        .sqrt();
    ensure!(norm > 0.0 && norm.is_finite(), "zero embedding");
    for v in &mut embedding {
        *v = (f64::from(*v) / norm) as f32;
    }
    Ok(embedding)
}
pub fn literal(vector: &[f32]) -> Result<String> {
    ensure!(
        vector.len() == DIMS && vector.iter().all(|v| v.is_finite()),
        "invalid search vector"
    );
    Ok(serde_json::to_string(vector)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_vectors_are_checked_and_normalized_before_storage() {
        assert!(normalize(vec![1.0; 1024]).is_err());
        assert!(normalize(vec![0.0; 4096]).is_err());
        let mut bad = vec![1.0; 4096];
        bad[4095] = f32::NAN;
        assert!(normalize(bad).is_err());
        let out = normalize(vec![2.0; 4096]).unwrap();
        assert_eq!(out.len(), 1024);
        assert!((out.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.00001);
    }
}
