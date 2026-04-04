# Module: agent-tools

**Crate:** `oneclick-agent-tools`
**Path:** `backend/crates/agent-tools/`
**Role:** Defines tools available to agents + JavaScript plugin for OpenClaw.

## Two Parts

### 1. Rust: Tool Definitions
- `ToolDefinition` struct: name, description, JSON Schema parameters
- `available_tools()` → Vec of 4 tools
- `generate_tool_config(backend_url, agent_id, user_id)` → JSON config for container mounting

### 2. JavaScript: OpenClaw Plugin
**File:** `plugin/oneclick-tools.js`

Registers 4 HTTP-based tools that call backend internal API:

| Tool | Endpoint | Method |
|------|----------|--------|
| `create_schedule` | `/internal/schedules` | POST |
| `list_schedules` | `/internal/schedules` | GET |
| `delete_schedule` | `/internal/schedules/{id}` | DELETE |
| `send_notification` | `/internal/notifications` | POST |

All requests include `X-Agent-Id` and `X-User-Id` headers from env vars.

## Tests
- `test_available_tools_count` — verifies 4 tools registered
- `test_generate_tool_config_structure` — verifies JSON config shape

## Extension
- New tool: add to `available_tools()` in Rust + add function + entry in JS plugin
- Tool authorization: add per-tool permission checks in internal endpoints
