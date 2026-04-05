//! OneClick.ai — LLM provider proxy and token tracking
//!
//! Routes OpenAI-compatible chat completion requests through a multi-provider
//! fallback chain (Groq → OpenRouter) with per-user rate limiting and
//! token usage logging to PostgreSQL.
//!
//! # Fallback chain
//!
//! 1. **Groq** `llama-3.3-70b-versatile`
//! 2. **Groq** `llama-3.1-8b-instant`
//! 3. **OpenRouter** `nvidia/llama-3.1-nemotron-70b-instruct:free`

pub mod provider;
pub mod service;
pub mod types;

// Re-export primary types for convenience.
pub use provider::{GroqProvider, LlmProvider, OpenRouterProvider, ProviderError};
pub use service::{LlmProxy, ProviderConfig};
pub use types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, TokenUsage,
};
