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
use uuid::Uuid;

/// Token usage for one AI op, parsed from the OpenAI-compatible `usage` block.
/// Embeddings responses carry only `prompt_tokens`; chat carries both. Missing
/// fields default to 0 (some local providers omit usage entirely).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Usage {
    #[serde(rename = "prompt_tokens", default)]
    pub input_tokens: i32,
    #[serde(rename = "completion_tokens", default)]
    pub output_tokens: i32,
}

/// The embedding dimension baked into `migrations/0016_ai_pgvector.sql`
/// (`vector(768)`). The default embed model (`nomic-embed-text`) is 768-dim.
/// Changing the model to another dimension needs a migration + re-embed.
pub const EMBED_DIM: usize = 768;

#[derive(Clone, Debug)]
pub struct AiConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Optional separate endpoint/key for embeddings, falling back to
    /// `base_url`/`api_key`. Lets chat go to a cloud provider that has no
    /// embeddings API (e.g. Kimi/Moonshot) while embeddings stay on local
    /// Ollama — mixed-provider is pure config, still no code change.
    pub embed_base_url: Option<String>,
    pub embed_api_key: Option<String>,
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
            embed_base_url: var("AI_EMBED_BASE_URL"),
            embed_api_key: var("AI_EMBED_API_KEY"),
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
    #[serde(default)]
    usage: Usage,
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
    #[serde(default)]
    usage: Usage,
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

    /// Embeddings endpoint/key — separate provider when `AI_EMBED_BASE_URL`
    /// is set, otherwise the shared `AI_BASE_URL` one.
    fn embed_endpoint(&self) -> String {
        let base = self.cfg.embed_base_url.as_deref().unwrap_or(&self.cfg.base_url);
        format!("{}/embeddings", base.trim_end_matches('/'))
    }

    fn embed_auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        // When a separate embed endpoint is configured, never fall back to the
        // chat key — that would send the cloud provider's credential to a
        // different host. No AI_EMBED_API_KEY on a separate host means no auth
        // header (the local-Ollama case).
        let key = if self.cfg.embed_base_url.is_some() {
            self.cfg.embed_api_key.as_ref()
        } else {
            self.cfg.embed_api_key.as_ref().or(self.cfg.api_key.as_ref())
        };
        match key {
            Some(k) => rb.bearer_auth(k),
            None => rb,
        }
    }

    /// Embed a batch of texts. Returns one vector per input (in input order)
    /// plus the token usage reported by the provider (for `ai_usage` metering).
    pub async fn embed(&self, inputs: &[String]) -> Result<(Vec<Vec<f32>>, Usage)> {
        if inputs.is_empty() {
            return Ok((vec![], Usage::default()));
        }
        let url = self.embed_endpoint();
        let resp = self
            .embed_auth(self.http.post(&url))
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
        let usage = parsed.usage;
        // Some providers don't guarantee response order; sort by index defensively.
        parsed.data.sort_by_key(|d| d.index);
        let out: Vec<Vec<f32>> = parsed.data.into_iter().map(|d| d.embedding).collect();
        if out.len() != inputs.len() {
            bail!("embedding count mismatch: got {} for {} inputs", out.len(), inputs.len());
        }
        Ok((out, usage))
    }

    /// Single non-streaming chat completion (used for summaries/tags). Returns
    /// the message content plus token usage (for `ai_usage` metering). Streaming
    /// for the RAG "ask" endpoint lands in P1.
    pub async fn chat(&self, system: &str, user: &str) -> Result<(String, Usage)> {
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
        let usage = parsed.usage;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("chat response had no choices")?;
        Ok((content, usage))
    }
}

/// Record one billable AI op into `ai_usage` (→ ONEWEB billing). Best-effort:
/// metering must never fail a request, so errors are logged and swallowed.
pub async fn record_usage(
    db: &sqlx::PgPool,
    user_id: Option<&str>,
    system_id: Option<&str>,
    op: &str,
    model: &str,
    usage: &Usage,
) {
    if let Err(e) = sqlx::query(
        r#"INSERT INTO ai_usage (id, user_id, system_id, op, model, input_tokens, output_tokens)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(system_id)
    .bind(op)
    .bind(model)
    .bind(usage.input_tokens)
    .bind(usage.output_tokens)
    .execute(db)
    .await
    {
        tracing::warn!("ai_usage insert failed (op={op}): {e}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_literal_formats_pgvector() {
        assert_eq!(vector_literal(&[]), "[]");
        assert_eq!(vector_literal(&[1.0]), "[1]");
        assert_eq!(vector_literal(&[0.5, -0.25, 2.0]), "[0.5,-0.25,2]");
    }

    #[test]
    fn usage_parses_chat_shape() {
        // OpenAI chat usage carries both token counts.
        let u: Usage = serde_json::from_str(
            r#"{"prompt_tokens":120,"completion_tokens":45,"total_tokens":165}"#,
        )
        .unwrap();
        assert_eq!(u.input_tokens, 120);
        assert_eq!(u.output_tokens, 45);
    }

    #[test]
    fn usage_parses_embedding_shape_without_completion() {
        // Embeddings usage has no completion_tokens — it must default to 0.
        let u: Usage =
            serde_json::from_str(r#"{"prompt_tokens":8,"total_tokens":8}"#).unwrap();
        assert_eq!(u.input_tokens, 8);
        assert_eq!(u.output_tokens, 0);
    }

    #[test]
    fn usage_defaults_when_provider_omits_it() {
        // Some local providers omit usage entirely; both counts default to 0.
        let u: Usage = serde_json::from_str("{}").unwrap();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
    }
}
