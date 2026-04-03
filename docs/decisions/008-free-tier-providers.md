# ADR-008: Free Tier via Groq + OpenRouter

## Status
Accepted

## Context
We need to provide AI agent access to 100 free-tier users at 50 requests/day each (5,000 requests/day total) without spending money on LLM inference.

## Decision
Use **Groq** as primary provider and **OpenRouter** as fallback, both on their free tiers.

### Provider breakdown

| Provider | Model | Free Quota | Quality | Speed |
|----------|-------|-----------|---------|-------|
| Groq | Llama 3.3 70B Versatile | 1,000 req/day | Very good | ~500 tok/s |
| Groq | Llama 3.1 8B Instant | 14,400 req/day | Good | ~1000 tok/s |
| OpenRouter | Nemotron 9B (:free) | 50 req/day | Decent | ~100 tok/s |

**Total free capacity: ~15,450 requests/day**
**Needed: 5,000 requests/day (100 users × 50)**
**Headroom: 3x**

### Routing strategy (ordered fallback)
1. **Groq Llama 3.3 70B** — best quality, use first
2. **Groq Llama 3.1 8B** — when 70B quota exhausted
3. **OpenRouter Nemotron** — emergency fallback

### Why these providers

**Groq advantages:**
- Fastest inference (custom LPU hardware, ~500 tokens/sec)
- Generous free tier (no credit card required)
- Stable quotas — haven't been reduced since 2025
- OpenAI-compatible API format

**OpenRouter advantages:**
- Access to 100+ models through one API
- Free model variants available (`:free` suffix)
- Already integrated from Phase 0

### Alternatives considered

**Google Gemini**: Free tier slashed to 20 req/day for Flash model (Dec 2025). Unreliable for a service.

**GitHub Copilot subscription**: Violates ToS to use as a backend API. Risk of account termination.

**Self-hosted Ollama**: Requires GPU. CPU inference too slow for OpenClaw's 16K minimum context (tested: 217s per response on CPU).

## Consequences
- No cost for LLM inference (Phase 1)
- Quality depends on Llama 3.3 70B (very good but not GPT-4/Claude level)
- Provider rate limits are daily — uneven usage patterns could exhaust early in the day
- If Groq changes free tier terms, need to adapt quickly (OpenRouter as backup)
- Paid tier (future): users bring their own API key or we fund via subscription revenue
