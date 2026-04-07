#!/usr/bin/env node
// =============================================================================
// OneClick.ai - Device Auto-Pairing Daemon (WebSocket)
// =============================================================================
// Connects to the OpenClaw gateway via WebSocket and auto-approves any
// device pairing requests. Runs persistently so both the chat-bridge
// and browser sessions are approved automatically.
// =============================================================================

const crypto = require("crypto");
const WebSocket = require("/app/node_modules/ws");

const GW_PORT = process.env.AGENT_PORT || 3000;
const GW_TOKEN = process.env.OPENCLAW_GATEWAY_TOKEN || "oneclick-local-dev";
const RECONNECT_BASE_MS = 2000;
const RECONNECT_MAX_MS = 10000;
const LIST_POLL_INTERVAL = 3000;

// Unique device identity for this daemon
const keypair = crypto.generateKeyPairSync("ed25519");
const rawPubKey = keypair.publicKey
  .export({ type: "spki", format: "der" })
  .slice(-32);
const deviceId = crypto
  .createHash("sha256")
  .update(rawPubKey)
  .digest("hex");
const publicKeyB64 = rawPubKey.toString("base64url");

let ws = null;
let connected = false;
let reconnectMs = RECONNECT_BASE_MS;
let reconnectTimer = null;
let reqCounter = 0;
let listInterval = null;

function nextId() {
  return `pair-${++reqCounter}`;
}

function send(obj) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(obj));
  }
}

// Approve a single pending device
function approveDevice(requestId) {
  console.log("[pair-device] Approving device request:", requestId);
  send({ type: "req", id: nextId(), method: "device.pair.approve", params: { requestId } });
}

// Request the list of pending devices
function listDevices() {
  if (!connected) return;
  send({ type: "req", id: `_list-${++reqCounter}`, method: "device.pair.list", params: {} });
}

function connect() {
  if (ws) {
    try { ws.close(); } catch {}
  }
  connected = false;
  if (listInterval) { clearInterval(listInterval); listInterval = null; }

  const sock = new WebSocket(`ws://127.0.0.1:${GW_PORT}/?token=${GW_TOKEN}`, {
    headers: { Origin: "http://127.0.0.1:" + GW_PORT },
  });
  ws = sock;

  sock.on("open", () => {
    console.log("[pair-device] WS opened, waiting for challenge...");
  });

  sock.on("message", (data) => {
    let msg;
    try { msg = JSON.parse(data.toString()); } catch { return; }

    // Handle connect challenge
    if (msg.type === "event" && msg.event === "connect.challenge") {
      const nonce = msg.payload.nonce;
      const scopes = [
        "operator.admin", "operator.read", "operator.write",
        "operator.approvals", "operator.pairing",
      ];
      const signedAt = Date.now();
      const payload = [
        "v2", deviceId, "openclaw-control-ui", "webchat", "operator",
        scopes.join(","), String(signedAt), GW_TOKEN, nonce,
      ].join("|");
      const signature = crypto
        .sign(null, Buffer.from(payload, "utf8"), keypair.privateKey)
        .toString("base64url");

      send({
        type: "req", id: "_connect", method: "connect",
        params: {
          minProtocol: 3, maxProtocol: 3,
          client: {
            id: "openclaw-control-ui", platform: "linux",
            mode: "webchat", version: "2026.3.13",
            instanceId: `pair-daemon-${process.pid}`,
          },
          role: "operator", scopes,
          device: { id: deviceId, publicKey: publicKeyB64, signature, signedAt, nonce },
          auth: { token: GW_TOKEN },
          caps: [],
        },
      });
    }

    // Handle connect response
    if (msg.type === "res" && msg.id === "_connect") {
      if (msg.ok) {
        connected = true;
        reconnectMs = RECONNECT_BASE_MS;
        console.log("[pair-device] Connected — watching for pairing requests...");
        // Poll pending list periodically
        listDevices();
        listInterval = setInterval(listDevices, LIST_POLL_INTERVAL);
      } else {
        console.error("[pair-device] Connect failed:", msg.error?.message);
        scheduleReconnect();
      }
      return;
    }

    // Handle device.pair.list response — approve all pending
    if (msg.type === "res" && msg.id?.startsWith("_list-") && msg.ok) {
      const pending = msg.result?.pending || [];
      for (const req of pending) {
        const rid = req.requestId || req.id;
        if (rid) approveDevice(rid);
      }
    }

    // Handle real-time pairing request event — approve immediately
    if (msg.type === "event" && msg.event === "device.pair.requested") {
      const rid = msg.payload?.requestId || msg.payload?.id;
      if (rid) approveDevice(rid);
    }
  });

  sock.on("error", (e) => {
    // Suppress connection refused during gateway boot
    if (!e.message?.includes("ECONNREFUSED")) {
      console.error("[pair-device] WS error:", e.message);
    }
  });

  sock.on("close", () => {
    connected = false;
    ws = null;
    if (listInterval) { clearInterval(listInterval); listInterval = null; }
    scheduleReconnect();
  });
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    reconnectMs = Math.min(reconnectMs * 2, RECONNECT_MAX_MS);
    connect();
  }, reconnectMs);
}

console.log("[pair-device] Starting auto-pairing daemon...");
connect();
