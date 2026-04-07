#!/usr/bin/env node
// =============================================================================
// OneClick.ai - Device Auto-Pairing Script
// =============================================================================
// Polls the OpenClaw gateway for pending device-pairing requests and
// auto-approves them so the backend can immediately use CLI write access.
// Runs as a background process launched by entrypoint.sh.
// =============================================================================

const http = require("http");
const fs = require("fs");

const GW_PORT = process.env.AGENT_PORT || 3000;
const GW_TOKEN = process.env.OPENCLAW_GATEWAY_TOKEN || "oneclick-local-dev";
const MARKER = "/home/node/.openclaw/device-paired";
const MAX_ATTEMPTS = 120;
const POLL_INTERVAL = 3000;

function request(method, urlPath, body) {
  return new Promise((resolve, reject) => {
    const opts = {
      hostname: "127.0.0.1",
      port: GW_PORT,
      path: urlPath,
      method,
      headers: {
        Authorization: "Bearer " + GW_TOKEN,
        "Content-Type": "application/json",
      },
    };
    const req = http.request(opts, (res) => {
      let data = "";
      res.on("data", (chunk) => (data += chunk));
      res.on("end", () => {
        try {
          resolve({ status: res.statusCode, body: JSON.parse(data) });
        } catch (e) {
          resolve({ status: res.statusCode, body: data });
        }
      });
    });
    req.on("error", reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

async function waitForGateway() {
  for (let i = 0; i < MAX_ATTEMPTS; i++) {
    try {
      const res = await request("GET", "/");
      if (res.status === 200) return true;
    } catch (e) {}
    await new Promise((r) => setTimeout(r, POLL_INTERVAL));
  }
  return false;
}

async function approvePendingDevices() {
  try {
    const res = await request("GET", "/api/devices");
    const pending = res.body.filter(
      (d) => d.status === "pending" || d.state === "pending"
    );
    for (const device of pending) {
      const id = device.id || device.deviceId;
      console.log("[pair-device] Approving device " + id);
      await request("POST", "/api/devices/" + id + "/approve", {});
    }
    return pending.length > 0;
  } catch (e) {
    return false;
  }
}

async function main() {
  if (fs.existsSync(MARKER)) {
    console.log("[pair-device] Already paired, exiting.");
    return;
  }

  console.log("[pair-device] Waiting for gateway...");
  const ready = await waitForGateway();
  if (!ready) {
    console.log("[pair-device] Gateway not ready after timeout, exiting.");
    return;
  }
  console.log("[pair-device] Gateway ready, polling for devices...");

  for (let i = 0; i < 10; i++) {
    const approved = await approvePendingDevices();
    if (approved) {
      fs.writeFileSync(MARKER, new Date().toISOString());
      return;
    }
    await new Promise((r) => setTimeout(r, POLL_INTERVAL));
  }
  console.log("[pair-device] No pending devices found. Done.");
}

main().catch((err) => {
  console.error("[pair-device] Error:", err.message);
  process.exit(0);
});
