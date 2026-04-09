# ADR-005: LLM Proxy over Direct Provider Access

## Status
Accepted

## Context
Agent containers need to call LLM APIs (Groq, OpenRouter) for inference. Two options:
1. Agents call providers directly (API key in each VM's env)
2. Agents call our backend proxy, which routes to providers

## Decision
All LLM calls from agents go through an **LLM proxy** endpoint in the backend.

```
Agent VM → POST http://172.16.0.{host-ip}:8080/internal/llm/v1/chat/completions
  → Backend routes to Groq (primary) → OpenRouter (fallback)
  → Backend logs usage → returns response to agent
```

Agent's OpenClaw config points to our proxy as the "OpenRouter" base URL:
```
OPENROUTER_BASE_URL=http://172.16.0.{host-ip}:8080/internal/llm/v1
```

(For Docker runtime, agents reach the backend at `http://host.docker.internal:8080`)

## Rationale

### Why proxy
- **Tamper-proof usage tracking**: Backend sees every LLM call, logs exact token counts
- **Rate limiting**: Enforce per-user limits before the call reaches the provider
- **Provider swapping**: Change Groq → OpenAI → Anthropic without touching any agent container
- **Fallback chains**: Groq rate limited? Automatically try OpenRouter. Transparent to agent.
- **Single API key**: One Groq key, one OpenRouter key — not duplicated across N containers
- **Cost control**: Can cap spending per user at the proxy layer

### Why not LiteLLM
- Our routing logic is simple (ordered fallback of 2-3 providers)
- All providers use OpenAI-compatible API format
- ~200 lines of Rust vs a separate container dependency
- LiteLLM makes sense for multi-team setups with 10+ providers — overkill here
- See ADR-006

### Fallback strategy
```
1. Groq (Llama 3.3 70B)     — best quality, 1,000/day
2. Groq (Llama 3.1 8B)      — more quota, 14,400/day
3. OpenRouter (Nemotron free) — last resort, 50/day
```
Total: ~15,450 free requests/day → 100 users × 50 req/day with 3x headroom.

## Consequences
- All LLM traffic routes through backend (single point of failure for inference)
- Slight latency overhead (~5ms per proxy hop)
- Backend must handle streaming responses (SSE passthrough)
- API keys only stored in backend, not in agent VMs
- MAX_MESSAGE_CHARS = 200,000 (supports large context windows)
- OpenClaw contextTokens set to 65,536
