//! LLM proxy service with ordered fallback chain, rate limiting, and usage logging.
//!
//! The [`LlmProxy`] routes chat completion requests through multiple LLM providers
//! in priority order. If a provider is rate-limited or errors out, the next
//! provider/model combination in the chain is tried automatically.
//!
//! **Default fallback chain:**
//! 1. Groq — `llama-3.3-70b-versatile`
//! 2. Groq — `llama-3.1-8b-instant`
//! 3. OpenRouter — `nvidia/llama-3.1-nemotron-70b-instruct:free`

use sqlx::PgPool;
use uuid::Uuid;

use oneclick_shared::config::Config;
use oneclick_shared::errors::{AppError, AppResult};

use crate::provider::{send_openai_request, ProviderError};
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage};

// ---------------------------------------------------------------------------
// Message truncation
// ---------------------------------------------------------------------------

/// Truncate messages to keep total content under `max_chars`.
///
/// Strategy: keep the system message (first) and latest user message (last),
/// trim middle messages, and truncate individual message content if needed.
fn truncate_messages(messages: &mut Vec<ChatMessage>, max_chars: usize) {
    // Calculate total character count across all messages
    let total: usize = messages.iter().map(|m| content_len(&m.content)).sum();
    if total <= max_chars {
        return;
    }

    // Drop middle messages first (keep first + last 2)
    while messages.len() > 3 && char_total(messages) > max_chars {
        messages.remove(1);
    }

    // If still over, truncate the longest message content
    if char_total(messages) > max_chars {
        for msg in messages.iter_mut() {
            truncate_content(&mut msg.content, 2000);
        }
    }
}

fn content_len(val: &serde_json::Value) -> usize {
    match val {
        serde_json::Value::String(s) => s.len(),
        serde_json::Value::Array(arr) => arr.iter().map(|v| content_len(v)).sum(),
        _ => val.to_string().len(),
    }
}

fn char_total(messages: &[ChatMessage]) -> usize {
    messages.iter().map(|m| content_len(&m.content)).sum()
}

/// Maximum total character count for message content sent to providers.
/// Rough heuristic (~2-3 chars per token) to stay within TPM limits.
const MAX_MESSAGE_CHARS: usize = 8000;

fn truncate_content(val: &mut serde_json::Value, max: usize) {
    match val {
        serde_json::Value::String(s) => {
            if s.len() > max {
                let truncate_at = s.floor_char_boundary(max);
                s.truncate(truncate_at);
                s.push_str("...[truncated]");
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                truncate_content(item, max);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(text) = map.get_mut("text") {
                truncate_content(text, max);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// ProviderConfig
// ---------------------------------------------------------------------------

/// Configuration for a single LLM provider in the fallback chain.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Provider identifier (e.g., `"groq"`, `"openrouter"`).
    pub name: String,
    /// Base URL for the provider's OpenAI-compatible API.
    pub base_url: String,
    /// API key for authentication.
    pub api_key: String,
    /// Models to try on this provider, in order.
    pub models: Vec<String>,
}

// ---------------------------------------------------------------------------
// LlmProxy
// ---------------------------------------------------------------------------

/// Multi-provider LLM proxy with ordered fallback and usage tracking.
///
/// Agent containers send OpenAI-compatible requests to this proxy. The proxy
/// iterates through configured providers and models until one succeeds, then
/// logs token usage to PostgreSQL.
pub struct LlmProxy {
    /// Ordered list of provider configurations (tried in sequence).
    providers: Vec<ProviderConfig>,
    /// Shared HTTP client for all provider requests.
    client: reqwest::Client,
    /// PostgreSQL connection pool for usage logging and rate-limit checks.
    db: PgPool,
}

impl LlmProxy {
    /// Create a new LLM proxy from application config and database pool.
    ///
    /// Configures the default fallback chain:
    /// 1. **Groq** — `llama-3.3-70b-versatile`, `llama-3.1-8b-instant`
    /// 2. **OpenRouter** — `nvidia/llama-3.1-nemotron-70b-instruct:free`
    pub fn new(config: &Config, db: PgPool) -> Self {
        let providers = vec![
            ProviderConfig {
                name: "groq".to_string(),
                base_url: "https://api.groq.com/openai/v1".to_string(),
                api_key: config.groq_api_key.clone(),
                models: vec![
                    "llama-3.3-70b-versatile".to_string(),
                    "llama-3.1-8b-instant".to_string(),
                ],
            },
            ProviderConfig {
                name: "openrouter".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: config.openrouter_api_key.clone(),
                models: vec![
                    "nvidia/llama-3.1-nemotron-70b-instruct:free".to_string(),
                ],
            },
        ];

        tracing::info!(
            provider_count = providers.len(),
            "LLM proxy initialized with fallback chain"
        );

        Self {
            providers,
            client: reqwest::Client::new(),
            db,
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Route a chat completion request through the fallback chain.
    ///
    /// Iterates over every (provider, model) pair in priority order:
    /// - On success: logs token usage and returns the response.
    /// - On provider rate-limit (HTTP 429): warns and tries the next pair.
    /// - On any other error: logs and tries the next pair.
    ///
    /// Returns [`AppError::Internal`] if every provider/model combination fails.
    pub async fn chat_completion(
        &self,
        user_id: Uuid,
        agent_id: Uuid,
        request: ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse> {
        let mut last_error: Option<ProviderError> = None;

        for provider in &self.providers {
            for model in &provider.models {
                let mut req = request.clone();
                req.model = model.clone();
                // Strip ALL extra fields to avoid inflating the request body.
                req.extra = serde_json::Value::Object(serde_json::Map::new());
                // Force non-streaming — the proxy expects a single JSON response.
                req.stream = Some(false);
                // Truncate messages to stay within provider token limits.
                truncate_messages(&mut req.messages, MAX_MESSAGE_CHARS);

                match self.try_provider(provider, &req).await {
                    Ok(response) => {
                        let (tokens_in, tokens_out) = response
                            .usage
                            .as_ref()
                            .map(|u| (u.prompt_tokens, u.completion_tokens))
                            .unwrap_or((0, 0));

                        tracing::info!(
                            provider = %provider.name,
                            model = %response.model,
                            prompt_tokens = tokens_in,
                            completion_tokens = tokens_out,
                            "Chat completion succeeded"
                        );

                        // Always log usage so rate-limit accounting is accurate.
                        self.log_usage(
                            user_id,
                            agent_id,
                            &response.model,
                            &provider.name,
                            tokens_in,
                            tokens_out,
                        )
                        .await?;

                        return Ok(response);
                    }
                    Err(ProviderError::RateLimited(msg)) => {
                        tracing::warn!(
                            provider = %provider.name,
                            model = %model,
                            "Rate limited by provider: {}", msg
                        );
                        last_error = Some(ProviderError::RateLimited(msg));
                    }
                    Err(e) => {
                        tracing::error!(
                            provider = %provider.name,
                            model = %model,
                            error = %e,
                            "Provider failed, trying next"
                        );
                        last_error = Some(e);
                    }
                }
            }
        }

        let detail = last_error
            .map(|e| format!("last error: {e}"))
            .unwrap_or_else(|| "no providers configured".to_string());

        Err(AppError::Internal(format!(
            "All LLM providers failed ({detail})"
        )))
    }

    /// Check whether a user is within their daily request limit.
    ///
    /// - **Pro-tier** users are unlimited (always returns `Ok`).
    /// - **Free-tier** users are checked against `daily_limit` by counting
    ///   today's rows in the `usage` table.
    pub async fn check_rate_limit(
        &self,
        user_id: Uuid,
        tier: &str,
        daily_limit: u32,
    ) -> AppResult<()> {
        if tier == "pro" {
            return Ok(());
        }

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM usage WHERE user_id = $1 AND created_at >= date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'",
        )
        .bind(user_id)
        .fetch_one(&self.db)
        .await?;

        if count.0 >= daily_limit as i64 {
            tracing::warn!(
                user_id = %user_id,
                tier = %tier,
                count = count.0,
                limit = daily_limit,
                "User exceeded daily rate limit"
            );
            return Err(AppError::RateLimited {
                limit: daily_limit,
                resets_at: "midnight UTC".to_string(),
            });
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Log token usage to PostgreSQL.
    async fn log_usage(
        &self,
        user_id: Uuid,
        agent_id: Uuid,
        model: &str,
        provider: &str,
        tokens_in: i32,
        tokens_out: i32,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO usage (user_id, agent_id, tokens_in, tokens_out, model, provider) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user_id)
        .bind(agent_id)
        .bind(tokens_in)
        .bind(tokens_out)
        .bind(model)
        .bind(provider)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Send a request to a provider endpoint, reusing the shared
    /// [`send_openai_request`] helper from the provider module.
    async fn try_provider(
        &self,
        provider: &ProviderConfig,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        send_openai_request(&self.client, &provider.base_url, &provider.api_key, request).await
    }
}
