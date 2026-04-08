# OneClick.ai Frontend

React 19 + TypeScript + Vite + Tailwind CSS + shadcn/ui

## Stack

- **React 19** with TypeScript
- **Vite** dev server with HMR
- **Tailwind CSS** for styling
- **shadcn/ui** component library
- **React Router** for client-side routing
- **Sonner** for toast notifications
- **Lucide** for icons

## Pages

| Page | Path | Description |
|------|------|-------------|
| Login | `/login` | Email/password auth |
| Signup | `/signup` | Account creation |
| Dashboard | `/dashboard` | Agent list with Wake/Chat/Sleep/Delete buttons |
| Chat | `/chat/:id` | Real-time chat with agent (WebSocket + token streaming) |
| Usage | `/usage` | LLM usage statistics |
| Schedules | `/schedules` | Cron job management |
| Notifications | `/notifications` | Alert inbox |

## Development

```bash
npm install
npm run dev    # starts on http://localhost:3000
```

## API Proxy

Vite is configured to proxy API calls to the backend (`vite.config.ts`):

| Frontend Path | Backend Target | Notes |
|---------------|---------------|-------|
| `/api/*` | `http://localhost:8080` | REST + WebSocket (ws: true) |
| `/agent-ui/*` | `http://localhost:8080` | OpenClaw gateway UI proxy |

This means the frontend and backend share the same origin during development — no CORS issues. Only port 3000 needs to be forwarded when developing on a remote VM.

## Dashboard UX

- **Create Agent**: Fire-and-forget — closes dialog immediately, shows "Creating…" card with pulsing status badge. Agent card updates via 5s auto-refresh polling.
- **Wake**: Click "Wake" button → shows "Waking…" spinner → updates to "Chat" when agent is running.
- **Chat**: Only enabled when agent status is `running`. Opens chat page which gates on gateway readiness before showing the chat UI.
- **Sleep**: Available when agent is running. Click "Sleep" → shows "Sleeping…" spinner → updates to "Wake" when stopped.
- **Delete**: Only available when agent is stopped. Confirms via dialog before destroying.

## Chat Flow

1. ChatPage fetches `GET /api/agents/:id` → if not running, redirects to dashboard
2. Polls `GET /api/agents/:id/gateway-status` every 3s until `{ "ready": true }`
3. Shows "Waiting for agent gateway…" loading screen during polling
4. Once ready, opens WebSocket at `ws://host/api/agents/:id/chat?token=jwt`
5. Streams tokens in real-time with typing indicator
