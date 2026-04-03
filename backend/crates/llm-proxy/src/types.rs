//! OpenAI-compatible request and response types for chat completions.
//!
//! These types mirror the OpenAI chat completions API schema, allowing the
//! proxy to communicate with any OpenAI-compatible provider (Groq, OpenRouter).

use serde::{Deserialize, Serialize};

/// OpenAI-compatible chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model identifier (overridden by the proxy's fallback chain).
    pub model: String,

    /// Conversation messages.
    pub messages: Vec<ChatMessage>,

    /// Sampling temperature (0.0-2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,

    /// Whether to stream the response via SSE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    /// Additional provider-specific fields passed through transparently.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// The role of the message author (`system`, `user`, `assistant`).
    pub role: String,

    /// The content of the message.
    pub content: String,
}

/// OpenAI-compatible chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    /// Unique identifier for the completion.
    pub id: String,

    /// Object type (always `"chat.completion"`).
    pub object: String,

    /// Unix timestamp of creation.
    pub created: i64,

    /// Model used for the completion.
    pub model: String,

    /// Generated choices.
    pub choices: Vec<Choice>,

    /// Token usage statistics (may be absent for streaming responses).
    pub usage: Option<TokenUsage>,
}

/// A single completion choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Index of this choice.
    pub index: i32,

    /// The generated message.
    pub message: ChatMessage,

    /// Reason the model stopped generating (`stop`, `length`, etc.).
    pub finish_reason: Option<String>,
}

/// Token usage statistics for a completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Tokens used in the prompt.
    pub prompt_tokens: i32,

    /// Tokens generated in the completion.
    pub completion_tokens: i32,

    /// Total tokens (prompt + completion).
    pub total_tokens: i32,
}
