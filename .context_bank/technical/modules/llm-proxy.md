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
3. OpenRouter: nemotron-70b:free   (50 req/day, last resort)
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

## Types (OpenAI-Compatible)
- `ChatCompletionRequest` — model, messages, temperature, max_tokens, stream, extra (flatten)
- `ChatCompletionResponse` — id, object, created, model, choices, usage
- `ChatMessage` — role, content
- `Choice` — index, message, finish_reason
- `TokenUsage` — prompt_tokens, completion_tokens, total_tokens

## Provider Implementations
Both `GroqProvider` and `OpenRouterProvider` use the shared `send_openai_request` helper. Only base_url and api_key differ.

## Extension
- New provider: add `ProviderConfig` entry in `LlmProxy::new()`
- Streaming: implement SSE passthrough (not yet implemented — Phase 1 uses non-streaming)
- Custom models: allow per-agent model override via request
