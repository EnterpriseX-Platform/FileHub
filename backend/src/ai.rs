//! AI provider — pluggable, **local-first**.
//!
//! One HTTP client against the **OpenAI-compatible** API shape, so the same code
//! talks to Ollama (default, fully local — zero data egress), vLLM, a LiteLLM
//! gateway, OpenAI, or Azure OpenAI. Swapping providers is pure config
//! (`AI_BASE_URL` + `AI_API_KEY` + model names) — no code change.
//!
//! See FILEHUB_AI_NATIVE_SPEC.md §4.1. We keep a concrete client (not a trait)
//! because the OpenAI shape *is* the abstraction; extract a trait only if a
//! genuinely non-OpenAI provider ever appears.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

/// The embedding dimension baked into `migrations/0016_ai_pgvector.sql`
/// (`vector(768)`). The default embed model (`nomic-embed-text`) is 768-dim.
/// Changing the model to another dimension needs a migration + re-embed.
pub const EMBED_DIM: usize = 768;

#[derive(Clone, Debug)]
pub struct AiConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: Option<String>,
    pub embed_model: String,
    pub embed_dim: usize,
    pub chat_model: String,
    pub max_context_tokens: usize,
    pub timeout_secs: u64,
}

impl AiConfig {
    /// Read config from env, defaulting to a fully local Ollama setup so a
    /// fresh checkout needs no cloud key and no external dependency.
    pub fn from_env() -> Self {
        fn var(k: &str) -> Option<String> {
            std::env::var(k).ok().filter(|v| !v.is_empty())
        }
        fn bool_var(k: &str, default: bool) -> bool {
            match var(k) {
                Some(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"),
                None => default,
            }
        }
        AiConfig {
            enabled: bool_var("AI_ENABLED", true),
            // Ollama's OpenAI-compatible endpoint.
            base_url: var("AI_BASE_URL").unwrap_or_else(|| "http://localhost:11434/v1".into()),
            api_key: var("AI_API_KEY"),
            embed_model: var("AI_EMBED_MODEL").unwrap_or_else(|| "nomic-embed-text".into()),
            embed_dim: var("AI_EMBED_DIM").and_then(|v| v.parse().ok()).unwrap_or(EMBED_DIM),
            chat_model: var("AI_CHAT_MODEL").unwrap_or_else(|| "qwen2.5".into()),
            max_context_tokens: var("AI_MAX_CONTEXT_TOKENS").and_then(|v| v.parse().ok()).unwrap_or(8192),
            timeout_secs: var("AI_TIMEOUT_SECS").and_then(|v| v.parse().ok()).unwrap_or(120),
        }
    }
}

#[derive(Clone)]
pub struct AiClient {
    cfg: AiConfig,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingDatum>,
}
#[derive(Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
    #[serde(default)]
    index: usize,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}
#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}
#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}

impl AiClient {
    pub fn new(cfg: AiConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .build()
            .expect("build reqwest client");
        if cfg.enabled && cfg.embed_dim != EMBED_DIM {
            tracing::warn!(
                "AI_EMBED_DIM={} but the pgvector column is vector({EMBED_DIM}). \
                 Embeddings will fail to insert until a migration widens the column. \
                 Use a {EMBED_DIM}-dim embedding model (e.g. nomic-embed-text) or migrate.",
                cfg.embed_dim
            );
        }
        Self { cfg, http }
    }

    pub fn enabled(&self) -> bool { self.cfg.enabled }
    pub fn embed_model(&self) -> &str { &self.cfg.embed_model }
    pub fn chat_model(&self) -> &str { &self.cfg.chat_model }
    pub fn embed_dim(&self) -> usize { self.cfg.embed_dim }
    pub fn max_context_tokens(&self) -> usize { self.cfg.max_context_tokens }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.cfg.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.cfg.api_key {
            Some(k) => rb.bearer_auth(k),
            None => rb,
        }
    }

    /// Embed a batch of texts. Returns one vector per input, in input order.
    pub async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }
        let url = self.endpoint("embeddings");
        let resp = self
            .auth(self.http.post(&url))
            .json(&json!({ "model": self.cfg.embed_model, "input": inputs }))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("embeddings request failed ({status}): {body}");
        }
        let mut parsed: EmbeddingResponse = resp.json().await.context("decode embeddings response")?;
        // Some providers don't guarantee response order; sort by index defensively.
        parsed.data.sort_by_key(|d| d.index);
        let out: Vec<Vec<f32>> = parsed.data.into_iter().map(|d| d.embedding).collect();
        if out.len() != inputs.len() {
            bail!("embedding count mismatch: got {} for {} inputs", out.len(), inputs.len());
        }
        Ok(out)
    }

    /// Single non-streaming chat completion (used for summaries/tags). Streaming
    /// for the RAG "ask" endpoint lands in P1.
    pub async fn chat(&self, system: &str, user: &str) -> Result<String> {
        let url = self.endpoint("chat/completions");
        let resp = self
            .auth(self.http.post(&url))
            .json(&json!({
                "model": self.cfg.chat_model,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
                "stream": false,
            }))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("chat request failed ({status}): {body}");
        }
        let parsed: ChatResponse = resp.json().await.context("decode chat response")?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("chat response had no choices")
    }
}

/// Format an embedding as a pgvector text literal (`[0.1,0.2,...]`) for binding
/// as a string and casting `::vector` in SQL — keeps us off the optional
/// pgvector-rust crate while staying exact. Matches the oneweb-control pattern.
pub fn vector_literal(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 8 + 2);
    s.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&x.to_string());
    }
    s.push(']');
    s
}
