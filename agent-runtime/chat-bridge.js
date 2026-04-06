/**
 * HTTP → WebSocket Chat Bridge (Persistent Connection)
 *
 * Runs inside the agent container on port 3001. Maintains a single
 * persistent WebSocket connection to the OpenClaw gateway, keeping
 * conversation context hot in memory across messages.
 *
 * POST /chat {"message":"…"} → SSE stream of tokens
 * GET  /health               → "ok"
 */

const http = require('http');
const crypto = require('crypto');
const WebSocket = require('/app/node_modules/.pnpm/ws@8.19.0/node_modules/ws');

const PORT = 3001;
const GW_TOKEN = process.env.OPENCLAW_GATEWAY_TOKEN || 'oneclick-internal';
const RECONNECT_BASE_MS = 500;
const RECONNECT_MAX_MS = 5000;

// Persistent Ed25519 keypair for this container instance
const keypair = crypto.generateKeyPairSync('ed25519');
const rawPubKey = keypair.publicKey
  .export({ type: 'spki', format: 'der' })
  .slice(-32);
const deviceId = crypto.createHash('sha256').update(rawPubKey).digest('hex');
const publicKeyB64 = rawPubKey.toString('base64url');

// ── Persistent gateway connection state ────────────────────────────────

let gatewayWs = null;
let isConnected = false;
let reconnectMs = RECONNECT_BASE_MS;
let reconnectTimer = null;
const pendingChats = new Map(); // requestId → { res, timeout }
let requestCounter = 0;

function nextRequestId() {
  return `chat-${++requestCounter}`;
}

function connectToGateway() {
  if (gatewayWs) {
    try { gatewayWs.close(); } catch {}
  }
  isConnected = false;

  const ws = new WebSocket(`ws://127.0.0.1:3000/?token=${GW_TOKEN}`, {
    headers: { Origin: 'http://127.0.0.1:3000' },
  });
  gatewayWs = ws;

  ws.on('open', () => {
    console.log('[bridge] WS opened, waiting for challenge...');
  });

  ws.on('message', (data) => {
    let msg;
    try { msg = JSON.parse(data.toString()); } catch { return; }

    // ── Connect handshake ──────────────────────────────────────────
    if (msg.type === 'event' && msg.event === 'connect.challenge') {
      const nonce = msg.payload.nonce;
      const scopes = [
        'operator.admin', 'operator.read', 'operator.write',
        'operator.approvals', 'operator.pairing',
      ];
      const signedAt = Date.now();
      const payload = [
        'v2', deviceId, 'openclaw-control-ui', 'webchat', 'operator',
        scopes.join(','), String(signedAt), GW_TOKEN, nonce,
      ].join('|');
      const signature = crypto
        .sign(null, Buffer.from(payload, 'utf8'), keypair.privateKey)
        .toString('base64url');

      ws.send(JSON.stringify({
        type: 'req', id: '_connect', method: 'connect',
        params: {
          minProtocol: 3, maxProtocol: 3,
          client: {
            id: 'openclaw-control-ui', platform: 'linux',
            mode: 'webchat', version: '2026.3.13',
            instanceId: `chat-bridge-${process.pid}`,
          },
          role: 'operator', scopes,
          device: { id: deviceId, publicKey: publicKeyB64, signature, signedAt, nonce },
          auth: { token: GW_TOKEN },
          caps: ['tool-events'],
        },
      }));
    }

    // ── Connect response ───────────────────────────────────────────
    if (msg.type === 'res' && msg.id === '_connect') {
      if (msg.ok) {
        isConnected = true;
        reconnectMs = RECONNECT_BASE_MS;
        console.log('[bridge] Connected to gateway (persistent, context hot)');
      } else {
        console.error('[bridge] Connect failed:', msg.error?.message);
        scheduleReconnect();
      }
      return;
    }

    // ── Chat send response ─────────────────────────────────────────
    if (msg.type === 'res' && msg.id?.startsWith('chat-')) {
      if (!msg.ok) {
        const pending = pendingChats.get(msg.id);
        if (pending) {
          clearTimeout(pending.timeout);
          pendingChats.delete(msg.id);
          const res = pending.res;
          if (!res.headersSent) {
            res.writeHead(500);
            res.end(JSON.stringify({ error: msg.error?.message || 'chat.send failed' }));
          } else {
            res.write(`data: ${JSON.stringify({ type: 'error', message: msg.error?.message })}\n\n`);
            res.write('data: [DONE]\n\n');
            res.end();
          }
        }
      }
      return;
    }

    // ── Chat streaming events ──────────────────────────────────────
    if (msg.type === 'event' && msg.event === 'chat') {
      const p = msg.payload || {};
      const runId = p.runId;

      // Find the pending chat for this run
      // Since we use idempotencyKey = requestId, match by checking all pending
      // OpenClaw doesn't echo our request ID in events, so we respond to
      // the most recent pending chat (FIFO for single-user bridge)
      let pendingEntry = null;
      let pendingKey = null;
      for (const [key, val] of pendingChats) {
        pendingEntry = val;
        pendingKey = key;
        break; // first (oldest) pending
      }

      if (!pendingEntry) return;
      const res = pendingEntry.res;

      if (p.state === 'delta') {
        const parts = p.message?.content || [];
        for (const part of parts) {
          if (part.type === 'text' && part.text) {
            res.write(`data: ${JSON.stringify({ type: 'stream', content: part.text })}\n\n`);
          }
        }
      }

      if (['final', 'error', 'aborted'].includes(p.state)) {
        const parts = p.message?.content || [];
        let text = '';
        for (const part of parts) {
          if (part.type === 'text') text += part.text;
        }

        if (p.state === 'error') {
          res.write(`data: ${JSON.stringify({ type: 'error', message: text || 'Agent error' })}\n\n`);
        } else {
          res.write(`data: ${JSON.stringify({ type: 'done', content: text })}\n\n`);
        }
        res.write('data: [DONE]\n\n');
        res.end();
        clearTimeout(pendingEntry.timeout);
        pendingChats.delete(pendingKey);
      }
    }
  });

  ws.on('error', (e) => {
    console.error('[bridge] WS error:', e.message);
  });

  ws.on('close', (code) => {
    console.log(`[bridge] WS closed (code=${code})`);
    isConnected = false;
    gatewayWs = null;
    // Fail all pending chats
    for (const [key, pending] of pendingChats) {
      clearTimeout(pending.timeout);
      if (!pending.res.headersSent) {
        pending.res.writeHead(503);
        pending.res.end(JSON.stringify({ error: 'Gateway disconnected' }));
      }
    }
    pendingChats.clear();
    scheduleReconnect();
  });
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  console.log(`[bridge] Reconnecting in ${reconnectMs}ms...`);
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    reconnectMs = Math.min(reconnectMs * 2, RECONNECT_MAX_MS);
    connectToGateway();
  }, reconnectMs);
}

// ── HTTP server ────────────────────────────────────────────────────────

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/health') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ status: 'ok', connected: isConnected }));
    return;
  }

  if (req.method === 'POST' && req.url === '/chat') {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      try {
        const { message } = JSON.parse(body);
        if (!message) {
          res.writeHead(400);
          res.end('{"error":"message required"}');
          return;
        }

        if (!isConnected || !gatewayWs) {
          res.writeHead(503);
          res.end('{"error":"Gateway not connected"}');
          return;
        }

        // SSE headers
        res.writeHead(200, {
          'Content-Type': 'text/event-stream',
          'Cache-Control': 'no-cache',
          'Connection': 'keep-alive',
        });

        const reqId = nextRequestId();
        const timeout = setTimeout(() => {
          pendingChats.delete(reqId);
          res.write(`data: ${JSON.stringify({ type: 'error', message: 'Timeout' })}\n\n`);
          res.write('data: [DONE]\n\n');
          res.end();
        }, 120_000);

        pendingChats.set(reqId, { res, timeout });

        gatewayWs.send(JSON.stringify({
          type: 'req',
          id: reqId,
          method: 'chat.send',
          params: {
            message,
            sessionKey: 'main',
            idempotencyKey: reqId,
          },
        }));
      } catch (e) {
        res.writeHead(400);
        res.end(JSON.stringify({ error: e.message }));
      }
    });
    return;
  }

  res.writeHead(404);
  res.end('Not found');
});

// ── Start ──────────────────────────────────────────────────────────────

server.listen(PORT, '0.0.0.0', () => {
  console.log(`[chat-bridge] Persistent bridge on port ${PORT}`);
  // Initial connection (gateway may not be ready yet — will auto-reconnect)
  setTimeout(connectToGateway, 5000);
});
