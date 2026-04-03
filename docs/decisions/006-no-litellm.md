# ADR-006: No LiteLLM

## Status
Accepted

## Context
LiteLLM is an open-source Python proxy that provides a unified API for 100+ LLM providers with features like load balancing, fallback, cost tracking, and rate limiting.

We considered using it as a sidecar container for LLM routing.

## Decision
**Do not use LiteLLM.** Build a simple LLM proxy module in the Rust backend instead.

## Rationale

### When LiteLLM makes sense
- Multi-team platform with 10+ LLM providers
- Need complex routing rules (A/B testing, canary, weighted distribution)
- Want a dedicated admin dashboard for LLM cost management
- Team doesn't want to maintain LLM routing code

### Why it's overkill for us
- We have 2-3 providers, all OpenAI-compatible → simple ordered fallback
- Usage tracking is a SQL INSERT — don't need a separate system
- Rate limiting is a Redis INCR — already in our backend
- Adding a Python sidecar container for ~200 lines of routing logic adds:
  - 80-150MB memory overhead
  - Another container to monitor and maintain
  - Python dependency management
  - An additional network hop

### Our alternative
```rust
// ~200 lines in the llm-proxy crate
for provider in [groq_70b, groq_8b, openrouter] {
    match provider.send(&request).await {
        Ok(resp) => { log_usage(); return Ok(resp); }
        Err(e) if e.is_rate_limited() => continue,
        Err(e) => return Err(e),
    }
}
```

### When we'd reconsider
- 10+ LLM providers with complex routing
- Need per-model A/B testing or canary deployments
- Multiple teams independently managing LLM access
- Basically: when we outgrow the simple fallback chain

## Consequences
- We maintain our own LLM routing (~200 lines of Rust)
- No dedicated LLM cost dashboard (usage tracked in our PostgreSQL, queryable via API)
- Adding a new provider = adding a new enum variant + base URL + API key
