//! LLM provider trait and implementations for Groq and OpenRouter.
//!
//! Each provider wraps an OpenAI-compatible API endpoint. The shared
//! [`send_openai_request`] helper handles the HTTP call, status checking,
//! and response deserialization so individual providers only need to supply
//! their base URL and API key.

use async_trait::async_trait;
use reqwest::Client;

use crate::types::{ChatCompletionRequest, ChatCompletionResponse};

// ---------------------------------------------------------------------------
// ProviderError
// ---------------------------------------------------------------------------

/// Errors that can occur when calling an LLM provider.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// The provider returned a 429 rate-limit response.
    #[error("Rate limited by provider: {0}")]
    RateLimited(String),

    /// The provider returned a non-success HTTP status.
    #[error("Provider error: {status} {body}")]
    ApiError { status: u16, body: String },

    /// A network or connection error occurred.
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// The provider returned a response that could not be parsed.
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

// ---------------------------------------------------------------------------
// LlmProvider trait
// ---------------------------------------------------------------------------

/// Trait for LLM providers that expose an OpenAI-compatible chat completions API.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Returns the provider name (e.g., `"groq"`, `"openrouter"`).
    fn name(&self) -> &str;

    /// Send a chat completion request and return the parsed response.
    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError>;
}

// ---------------------------------------------------------------------------
// Groq provider
// ---------------------------------------------------------------------------

/// Groq LLM provider (<https://api.groq.com/openai/v1>).
///
/// Supports Llama models via an OpenAI-compatible API.
pub struct GroqProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl GroqProvider {
    /// Create a new Groq provider with a shared HTTP client.
    pub fn new(client: Client, api_key: String) -> Self {
        Self {
            client,
            api_key,
            base_url: "https://api.groq.com/openai/v1".to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for GroqProvider {
    fn name(&self) -> &str {
        "groq"
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        send_openai_request(&self.client, &self.base_url, &self.api_key, request).await
    }
}

// ---------------------------------------------------------------------------
// OpenRouter provider
// ---------------------------------------------------------------------------

/// OpenRouter LLM provider (<https://openrouter.ai/api/v1>).
///
/// Provides access to many models, including free-tier options.
pub struct OpenRouterProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenRouterProvider {
    /// Create a new OpenRouter provider with a shared HTTP client.
    pub fn new(client: Client, api_key: String) -> Self {
        Self {
            client,
            api_key,
            base_url: "https://openrouter.ai/api/v1".to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        send_openai_request(&self.client, &self.base_url, &self.api_key, request).await
    }
}

// ---------------------------------------------------------------------------
// Shared request helper
// ---------------------------------------------------------------------------

/// Send an OpenAI-compatible chat completion request to the given endpoint.
///
/// This is the shared implementation used by every provider. It:
/// 1. POSTs the JSON request to `{base_url}/chat/completions`
/// 2. Maps HTTP 429 to [`ProviderError::RateLimited`]
/// 3. Maps other non-2xx statuses to [`ProviderError::ApiError`]
/// 4. Deserializes the successful response into [`ChatCompletionResponse`]
pub(crate) async fn send_openai_request(
    client: &Client,
    base_url: &str,
    api_key: &str,
    request: &ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ProviderError> {
    let url = format!("{}/chat/completions", base_url);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(request)
        .send()
        .await?;

    let status = response.status();

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let body = response.text().await.unwrap_or_default();
        return Err(ProviderError::RateLimited(body));
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(ProviderError::ApiError {
            status: status.as_u16(),
            body,
        });
    }

    let body = response.text().await.unwrap_or_default();
    tracing::trace!(body_len = body.len(), body_preview = %&body[..body.len().min(500)], "Raw provider response");
    let completion: ChatCompletionResponse = serde_json::from_str(&body)
        .map_err(|e| ProviderError::InvalidResponse(format!("{e}: {}", &body[..body.len().min(300)])))?;

    Ok(completion)
}
