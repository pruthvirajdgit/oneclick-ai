/**
 * HTTP → WebSocket Chat Bridge
 *
 * Runs inside the agent container on port 3001. Accepts POST /chat with
 * {"message":"…"}, connects to the local OpenClaw gateway via WebSocket
 * with Ed25519 device identity, and streams the response back as SSE.
 *
 * This sidesteps the "localhost-only pairing" restriction because we ARE
 * on localhost inside the container.
 */

const http = require('http');
const crypto = require('crypto');
// Use the ws module bundled with OpenClaw
const WebSocket = require('/app/node_modules/.pnpm/ws@8.19.0/node_modules/ws');

const PORT = 3001;
const GW_TOKEN = process.env.OPENCLAW_GATEWAY_TOKEN || 'oneclick-internal';

// Persistent Ed25519 keypair for this container instance
const keypair = crypto.generateKeyPairSync('ed25519');
const rawPubKey = keypair.publicKey
  .export({ type: 'spki', format: 'der' })
  .slice(-32);
const deviceId = crypto.createHash('sha256').update(rawPubKey).digest('hex');
const publicKeyB64 = rawPubKey.toString('base64url');

// ── Core chat logic ────────────────────────────────────────────────────

function connectAndChat(message, res) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://127.0.0.1:3000/?token=${GW_TOKEN}`, {
      headers: { Origin: 'http://127.0.0.1:3000' },
    });

    let settled = false;
    const timeout = setTimeout(() => {
      if (!settled) {
        settled = true;
        ws.close();
        reject(new Error('Timeout'));
      }
    }, 120_000);

    ws.on('message', (data) => {
      const msg = JSON.parse(data.toString());

      // ── Step 1: respond to the connect challenge ──────────────────
      if (msg.type === 'event' && msg.event === 'connect.challenge') {
        const nonce = msg.payload.nonce;
        const scopes = [
          'operator.admin',
          'operator.read',
          'operator.write',
          'operator.approvals',
          'operator.pairing',
        ];
        const signedAt = Date.now();
        const payload = [
          'v2',
          deviceId,
          'openclaw-control-ui',
          'webchat',
          'operator',
          scopes.join(','),
          String(signedAt),
          GW_TOKEN,
          nonce,
        ].join('|');
        const signature = crypto
          .sign(null, Buffer.from(payload, 'utf8'), keypair.privateKey)
          .toString('base64url');

        ws.send(
          JSON.stringify({
            type: 'req',
            id: 'c1',
            method: 'connect',
            params: {
              minProtocol: 3,
              maxProtocol: 3,
              client: {
                id: 'openclaw-control-ui',
                platform: 'linux',
                mode: 'webchat',
                version: '2026.3.13',
                instanceId: 'chat-bridge',
              },
              role: 'operator',
              scopes,
              device: {
                id: deviceId,
                publicKey: publicKeyB64,
                signature,
                signedAt,
                nonce,
              },
              auth: { token: GW_TOKEN },
              caps: ['tool-events'],
            },
          }),
        );
      }

      // ── Step 2: on successful connect, send the chat message ─────
      if (msg.type === 'res' && msg.id === 'c1') {
        if (msg.ok) {
          ws.send(
            JSON.stringify({
              type: 'req',
              id: 'ch1',
              method: 'chat.send',
              params: {
                message,
                sessionKey: 'main',
                idempotencyKey: crypto.randomUUID(),
              },
            }),
          );
        } else {
          settled = true;
          clearTimeout(timeout);
          reject(new Error(msg.error?.message || 'Connect failed'));
          ws.close();
        }
      }

      // ── chat.send rejection ──────────────────────────────────────
      if (msg.type === 'res' && msg.id === 'ch1' && !msg.ok) {
        settled = true;
        clearTimeout(timeout);
        reject(new Error(msg.error?.message || 'chat.send failed'));
        ws.close();
      }

      // ── Step 3: stream chat events as SSE ────────────────────────
      if (msg.type === 'event' && msg.event === 'chat') {
        const p = msg.payload || {};

        if (p.state === 'delta') {
          const parts = p.message?.content || [];
          for (const part of parts) {
            if (part.type === 'text' && part.text) {
              res.write(
                `data: ${JSON.stringify({ type: 'stream', content: part.text })}\n\n`,
              );
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
            res.write(
              `data: ${JSON.stringify({ type: 'error', message: text || 'Agent error' })}\n\n`,
            );
          } else {
            res.write(
              `data: ${JSON.stringify({ type: 'done', content: text })}\n\n`,
            );
          }
          res.write('data: [DONE]\n\n');
          res.end();
          settled = true;
          clearTimeout(timeout);
          ws.close();
          resolve();
        }
      }
    });

    ws.on('error', (e) => {
      if (!settled) {
        settled = true;
        clearTimeout(timeout);
        reject(e);
      }
    });
    ws.on('close', () => {
      if (!settled) {
        settled = true;
        clearTimeout(timeout);
        reject(new Error('WS closed'));
      }
    });
  });
}

// ── HTTP server ────────────────────────────────────────────────────────

const server = http.createServer(async (req, res) => {
  // Health check
  if (req.method === 'GET' && req.url === '/health') {
    res.writeHead(200);
    res.end('ok');
    return;
  }

  // Chat endpoint
  if (req.method === 'POST' && req.url === '/chat') {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', async () => {
      try {
        const { message } = JSON.parse(body);
        if (!message) {
          res.writeHead(400);
          res.end('{"error":"message required"}');
          return;
        }

        // SSE headers
        res.writeHead(200, {
          'Content-Type': 'text/event-stream',
          'Cache-Control': 'no-cache',
          Connection: 'keep-alive',
        });

        await connectAndChat(message, res);
      } catch (e) {
        if (!res.headersSent) {
          res.writeHead(500);
          res.end(JSON.stringify({ error: e.message }));
        } else {
          res.write(
            `data: ${JSON.stringify({ type: 'error', message: e.message })}\n\n`,
          );
          res.write('data: [DONE]\n\n');
          res.end();
        }
      }
    });
    return;
  }

  res.writeHead(404);
  res.end('Not found');
});

server.listen(PORT, '0.0.0.0', () => {
  console.log(`[chat-bridge] HTTP→WS bridge listening on port ${PORT}`);
});
