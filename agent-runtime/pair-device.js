#!/usr/bin/env node
// =============================================================================
// OneClick.ai - Device Auto-Pairing Daemon
// =============================================================================
// Runs as a persistent background process inside the agent container.
// Continuously polls the OpenClaw gateway for pending device-pairing requests
// and auto-approves them so that both the chat-bridge and browser sessions
// connect without manual intervention.
// =============================================================================

const http = require("http");

const GW_PORT = process.env.AGENT_PORT || 3000;
const GW_TOKEN = process.env.OPENCLAW_GATEWAY_TOKEN || "oneclick-local-dev";
const POLL_INTERVAL = 3000;
const GATEWAY_WAIT_INTERVAL = 5000;

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
  while (true) {
    try {
      const res = await request("GET", "/");
      if (res.status === 200) return;
    } catch (e) {}
    await new Promise((r) => setTimeout(r, GATEWAY_WAIT_INTERVAL));
  }
}

async function approvePendingDevices() {
  try {
    const res = await request("GET", "/api/devices");
    if (!Array.isArray(res.body)) return;
    const pending = res.body.filter(
      (d) => d.status === "pending" || d.state === "pending"
    );
    for (const device of pending) {
      const id = device.id || device.deviceId;
      console.log("[pair-device] Approving device " + id);
      await request("POST", "/api/devices/" + id + "/approve", {});
    }
  } catch (e) {
    // Gateway may have restarted — will retry next tick
  }
}

async function main() {
  console.log("[pair-device] Waiting for gateway...");
  await waitForGateway();
  console.log("[pair-device] Gateway ready — watching for new devices...");

  while (true) {
    await approvePendingDevices();
    await new Promise((r) => setTimeout(r, POLL_INTERVAL));
  }
}

main().catch((err) => {
  console.error("[pair-device] Fatal:", err.message);
  process.exit(1);
});
