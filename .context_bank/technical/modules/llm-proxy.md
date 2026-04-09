# Module: llm-proxy

**Crate:** `oneclick-llm-proxy`
**Path:** `backend/crates/llm-proxy/`
**Role:** Routes OpenAI-compatible LLM requests through a multi-provider fallback chain with usage tracking and rate limiting.

## Dependencies
`shared`, `reqwest`, `sqlx`

## Key Exports
- `LlmProxy` — service struct
- `ProviderConfig` — provider definition
- `ChatCompletionRequest/Response` — OpenAI-compatible types
- `LlmProvider` trait, `GroqProvider`, `OpenRouterProvider`
- `ProviderError` — rate limit, API error, network error

## Fallback Chain (Default)
```
1. Groq: llama-3.3-70b-versatile  (1,000 req/day, best quality)
2. Groq: llama-3.1-8b-instant     (14,400 req/day, good quality)
3. OpenRouter: free model           (last resort)
```

## LlmProxy Service
```rust
pub struct LlmProxy {
    providers: Vec<ProviderConfig>,
    client: reqwest::Client,
    db: PgPool,
}
```

### Methods
| Method | What it does |
|--------|-------------|
| `chat_completion(user_id, agent_id, request)` | Try each (provider, model) pair; log usage on success |
| `check_rate_limit(user_id, tier, daily_limit)` | Read-only pre-check: counts today's usage rows via Redis GET; pro users bypass. Called BEFORE request |
| `increment_rate_limit(user_id)` | Redis INCR counter; called AFTER successful LLM call only (prevents double-counting failures) |
| `log_usage(user_id, agent_id, model, provider, tokens_in, tokens_out)` | INSERT into usage table (always logs, even on zero tokens) |

### Fallback Logic
For each provider, for each model: try request. On 429 → warn + next. On error → error + next. On success → log usage + return. All failed → `AppError::Internal`.

### Request Processing
- `stream: false` is forced on all outgoing requests to ensure parseable JSON responses
- `truncate_messages(messages, MAX_MESSAGE_CHARS)` truncates content to stay within context limits (`MAX_MESSAGE_CHARS = 200,000` chars, increased from 8000 to support full context). Uses `floor_char_boundary` for UTF-8 safe truncation.
- Raw response body logged at `trace` level only (PII protection)

## Types (OpenAI-Compatible)
- `ChatCompletionRequest` — model, messages, temperature, max_tokens, stream. Extra fields (`extra`) are set to `None` before sending to providers to prevent payload pollution.
- `ChatCompletionResponse` — id, object, created, model, choices, usage
- `ChatMessage` — role, content (`serde_json::Value` — handles both string and array format for multimodal content from OpenClaw)
- `Choice` — index, message, finish_reason
- `TokenUsage` — prompt_tokens, completion_tokens, total_tokens

## Provider Implementations
Both `GroqProvider` and `OpenRouterProvider` use the shared `send_openai_request` helper. Only base_url and api_key differ.

## SSE Conversion Layer
When OpenClaw expects streaming (SSE format), the internal LLM endpoint in `internal.rs` converts the non-streaming JSON response into SSE events (`data: ...` chunks + `data: [DONE]`). This allows the proxy to always use `stream: false` upstream while satisfying clients that require SSE.

## Extension
- New provider: add `ProviderConfig` entry in `LlmProxy::new()`
- Custom models: allow per-agent model override via request
