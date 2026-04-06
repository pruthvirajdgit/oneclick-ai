#!/usr/bin/env python3
"""OneClick.ai E2E Test Suite — All 7 Tests"""

import asyncio
import json
import subprocess
import sys
import time
import urllib.request
import urllib.error

BASE = "http://localhost:8080"
RESULTS = {}

def api(method, path, token=None, body=None):
    url = BASE + path
    data = json.dumps(body).encode() if body else None
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            text = resp.read().decode()
            return resp.status, json.loads(text) if text else {}
    except urllib.error.HTTPError as e:
        body_text = e.read().decode() if e.fp else ""
        try:
            return e.code, json.loads(body_text)
        except:
            return e.code, {"error": body_text}
    except Exception as e:
        return 0, {"error": str(e)}

def psql(sql):
    result = subprocess.run(
        ["docker", "exec", "oneclick-ai-postgres-1", "psql", "-U", "oneclick", "-d", "oneclick", "-t", "-A", "-c", sql],
        capture_output=True, text=True, timeout=10
    )
    # Return only the first line (strips INSERT/UPDATE tags from RETURNING)
    lines = [l for l in result.stdout.strip().split("\n") if l and not l.startswith("INSERT") and not l.startswith("UPDATE") and not l.startswith("DELETE")]
    return lines[0] if lines else result.stdout.strip()

def docker_ps_filter(name_filter):
    result = subprocess.run(
        ["docker", "ps", "-a", "--filter", f"name={name_filter}", "--format", "{{.Names}}"],
        capture_output=True, text=True, timeout=10
    )
    return [x for x in result.stdout.strip().split("\n") if x]

def test_pass(name):
    RESULTS[name] = "PASS"
    print(f"  ✅ {name}: PASS")

def test_fail(name, reason):
    RESULTS[name] = f"FAIL: {reason}"
    print(f"  ❌ {name}: FAIL — {reason}")

# ============================================================
# TEST 1: Full Chat Roundtrip
# ============================================================
def test1_chat_roundtrip():
    print("\n=== TEST 1: Full Chat Roundtrip ===")

    # Signup
    status, data = api("POST", "/api/auth/signup", body={"email": "e2e@test.com", "password": "TestPass123!"})
    if status != 201:
        test_fail("test1", f"Signup failed: {status} {data}")
        return None, None, None
    token1 = data["token"]
    user1 = data["user"]["id"]
    print(f"  Signup OK: user={user1}")

    # Create agent
    status, data = api("POST", "/api/agents", token=token1, body={"model": "openrouter/auto"})
    if status != 201:
        test_fail("test1", f"Create agent failed: {status} {data}")
        return token1, user1, None
    agent1 = data["id"]
    print(f"  Agent created: {agent1}")

    # WebSocket chat
    import websockets
    async def do_chat():
        uri = f"ws://localhost:8080/api/agents/{agent1}/chat?token={token1}"
        print(f"  Connecting WebSocket...")
        async with websockets.connect(uri, open_timeout=180, ping_timeout=180) as ws:
            print("  Connected! Sending message...")
            await ws.send(json.dumps({"type": "message", "content": "Say hello in one word"}))
            for i in range(30):
                msg = await asyncio.wait_for(ws.recv(), timeout=180)
                data = json.loads(msg)
                t = data.get("type", "")
                body = data.get("message", "") or data.get("content", "")
                print(f"  [{t}] {str(body)[:200]}")
                if t == "done":
                    content = data.get("content", "")
                    if "error" in content.lower() or "HTTP 4" in content or "HTTP 5" in content:
                        return False, f"LLM error: {content[:100]}"
                    return True, content
                if t == "error":
                    if "wake" in str(body).lower() or "waking" in str(body).lower():
                        continue
                    return False, f"Error: {body}"
        return False, "No done message received"

    try:
        success, detail = asyncio.run(do_chat())
    except Exception as e:
        success, detail = False, str(e)

    if success:
        test_pass("test1")
    else:
        test_fail("test1", detail)

    return token1, user1, agent1

# ============================================================
# TEST 2: Sleep/Wake Cycle
# ============================================================
def test2_sleep_wake(token1, user1, agent1):
    print("\n=== TEST 2: Sleep/Wake Cycle ===")
    if not agent1:
        test_fail("test2", "No agent from test1")
        return

    # Find container
    containers = docker_ps_filter(f"agent-{user1[:8]}-{agent1[:8]}")
    if not containers:
        test_fail("test2", "Agent container not found")
        return
    container = containers[0]
    print(f"  Container: {container}")

    # Stop the container
    subprocess.run(["docker", "stop", container], capture_output=True, timeout=30)
    psql(f"UPDATE agents SET status = 'stopped' WHERE id = '{agent1}'")
    print("  Container stopped + DB updated")

    # Reconnect WebSocket — should wake
    import websockets
    async def do_wake():
        uri = f"ws://localhost:8080/api/agents/{agent1}/chat?token={token1}"
        print("  Reconnecting WebSocket...")
        async with websockets.connect(uri, open_timeout=180, ping_timeout=180) as ws:
            saw_waking = False
            saw_ready = False
            for i in range(30):
                msg = await asyncio.wait_for(ws.recv(), timeout=180)
                data = json.loads(msg)
                t = data.get("type", "")
                body = data.get("message", "") or data.get("content", "")
                print(f"  [{t}] {str(body)[:200]}")
                if t == "status" and "waking" in body.lower():
                    saw_waking = True
                if t == "status" and "ready" in body.lower():
                    saw_ready = True
                    return True, "Woke up successfully"
                if t == "error" and "wake" not in body.lower():
                    return False, f"Error: {body}"
            return saw_waking and saw_ready, f"waking={saw_waking} ready={saw_ready}"

    try:
        success, detail = asyncio.run(do_wake())
    except Exception as e:
        success, detail = False, str(e)

    if success:
        test_pass("test2")
    else:
        test_fail("test2", detail)

# ============================================================
# TEST 3: Agent Destroy + Cleanup
# ============================================================
def test3_destroy(token1):
    print("\n=== TEST 3: Agent Destroy + Cleanup ===")
    if not token1:
        test_fail("test3", "No token from test1")
        return

    # Create a second agent
    status, data = api("POST", "/api/agents", token=token1, body={"model": "openrouter/auto"})
    if status != 201:
        test_fail("test3", f"Create agent2 failed: {status} {data}")
        return
    agent2 = data["id"]
    print(f"  Agent2 created: {agent2}")

    # Get container name
    container_name = psql(f"SELECT container_name FROM agents WHERE id = '{agent2}'")
    print(f"  Container name: {container_name}")

    # Delete
    status, data = api("DELETE", f"/api/agents/{agent2}", token=token1)
    if status not in (200, 204):
        test_fail("test3", f"Delete failed: {status} {data}")
        return
    print("  Delete returned OK")

    time.sleep(2)

    # Verify container gone
    remaining = docker_ps_filter(container_name) if container_name else []
    if remaining:
        test_fail("test3", f"Container still exists: {remaining}")
        return

    # Verify DB record gone
    count = psql(f"SELECT COUNT(*) FROM agents WHERE id = '{agent2}'")
    if count != "0":
        test_fail("test3", f"DB record still exists (count={count})")
        return

    test_pass("test3")

# ============================================================
# TEST 4: Multi-Agent Isolation
# ============================================================
def test4_isolation(token1, user1, agent1):
    print("\n=== TEST 4: Multi-Agent Isolation ===")
    if not token1 or not agent1:
        test_fail("test4", "Missing token1 or agent1")
        return

    # Signup user2
    status, data = api("POST", "/api/auth/signup", body={"email": "e2e2@test.com", "password": "TestPass123!"})
    if status != 201:
        test_fail("test4", f"Signup user2 failed: {status} {data}")
        return
    token2 = data["token"]
    user2 = data["user"]["id"]
    print(f"  User2: {user2}")

    # Create agent for user2
    status, data = api("POST", "/api/agents", token=token2, body={"model": "openrouter/auto"})
    if status != 201:
        test_fail("test4", f"Create agent for user2 failed: {status} {data}")
        return
    agent2 = data["id"]
    print(f"  Agent2 (user2's): {agent2}")

    # User2 tries to access user1's agent
    status1, _ = api("GET", f"/api/agents/{agent1}", token=token2)
    print(f"  User2 accessing user1 agent: {status1}")
    if status1 != 404:
        test_fail("test4", f"Expected 404, got {status1} for user2->agent1")
        return

    # User1 tries to access user2's agent
    status2, _ = api("GET", f"/api/agents/{agent2}", token=token1)
    print(f"  User1 accessing user2 agent: {status2}")
    if status2 != 404:
        test_fail("test4", f"Expected 404, got {status2} for user1->agent2")
        return

    # Clean up user2's agent
    api("DELETE", f"/api/agents/{agent2}", token=token2)

    test_pass("test4")

# ============================================================
# TEST 5: Usage Endpoint
# ============================================================
def test5_usage(token1):
    print("\n=== TEST 5: Usage Endpoint ===")
    if not token1:
        test_fail("test5", "No token")
        return

    status, data = api("GET", "/api/usage", token=token1)
    print(f"  Response: {status} {json.dumps(data)[:300]}")
    if status != 200:
        test_fail("test5", f"Status {status}: {data}")
        return

    # Validate structure
    for key in ["today", "all_time"]:
        if key not in data:
            test_fail("test5", f"Missing '{key}' field")
            return
        section = data[key]
        for field in ["requests", "tokens_in", "tokens_out"]:
            if field not in section:
                test_fail("test5", f"Missing '{key}.{field}'")
                return

    if "limit" not in data["today"]:
        test_fail("test5", "Missing 'today.limit'")
        return

    test_pass("test5")

# ============================================================
# TEST 6: Notifications CRUD
# ============================================================
def test6_notifications(token1, user1):
    print("\n=== TEST 6: Notifications CRUD ===")
    if not token1:
        test_fail("test6", "No token")
        return

    # Step 1: Get notifications (should be empty)
    status, data = api("GET", "/api/notifications", token=token1)
    if status != 200:
        test_fail("test6", f"List failed: {status} {data}")
        return
    if len(data) != 0:
        test_fail("test6", f"Expected empty, got {len(data)} items")
        return
    print("  Step 1: Empty list OK")

    # Step 2: Insert via DB
    notif_id = psql(f"INSERT INTO notifications (user_id, title, body) VALUES ('{user1}', 'Test', 'Body') RETURNING id")
    if not notif_id:
        test_fail("test6", "DB insert failed")
        return
    print(f"  Step 2: Inserted notification id={notif_id}")

    # Step 3: Get notifications (should have 1)
    status, data = api("GET", "/api/notifications", token=token1)
    if status != 200 or len(data) != 1:
        test_fail("test6", f"Expected 1 item, got {status} {len(data) if isinstance(data, list) else data}")
        return
    print("  Step 3: 1 item OK")

    # Step 4: Mark as read
    status, _ = api("POST", f"/api/notifications/{notif_id}/read", token=token1)
    if status != 200:
        test_fail("test6", f"Mark read failed: {status}")
        return
    print("  Step 4: Mark read OK")

    # Step 5: Verify read=true
    status, data = api("GET", "/api/notifications", token=token1)
    if status != 200 or len(data) != 1:
        test_fail("test6", f"Expected 1 item after read, got {status}")
        return
    if not data[0].get("read"):
        test_fail("test6", f"read not true: {data[0]}")
        return
    print("  Step 5: read=true OK")

    test_pass("test6")

# ============================================================
# TEST 7: Schedules CRUD
# ============================================================
def test7_schedules(token1, agent1):
    print("\n=== TEST 7: Schedules CRUD ===")
    if not token1 or not agent1:
        test_fail("test7", "No token or agent")
        return

    # Step 1: Create schedule
    status, data = api("POST", "/api/schedules", token=token1,
        body={"agent_id": agent1, "cron_expr": "0 */3 * * *", "task_message": "test"})
    if status != 201:
        test_fail("test7", f"Create failed: {status} {data}")
        return
    sched_id = data.get("id")
    print(f"  Step 1: Created schedule {sched_id}")

    # Step 2: List schedules (should have 1)
    status, data = api("GET", "/api/schedules", token=token1)
    if status != 200 or len(data) != 1:
        test_fail("test7", f"Expected 1, got {status} {len(data) if isinstance(data, list) else data}")
        return
    print("  Step 2: 1 schedule OK")

    # Step 3: Delete
    status, _ = api("DELETE", f"/api/schedules/{sched_id}", token=token1)
    if status not in (200, 204):
        test_fail("test7", f"Delete failed: {status}")
        return
    print("  Step 3: Delete OK")

    # Step 4: List (should be empty)
    status, data = api("GET", "/api/schedules", token=token1)
    if status != 200:
        test_fail("test7", f"List after delete failed: {status}")
        return
    if len(data) != 0:
        test_fail("test7", f"Expected empty, got {len(data)}")
        return
    print("  Step 4: Empty list OK")

    test_pass("test7")


# ============================================================
# MAIN
# ============================================================
if __name__ == "__main__":
    print("=" * 60)
    print("OneClick.ai E2E Test Suite")
    print("=" * 60)

    token1, user1, agent1 = test1_chat_roundtrip()
    test2_sleep_wake(token1, user1, agent1)
    test3_destroy(token1)
    test4_isolation(token1, user1, agent1)
    test5_usage(token1)
    test6_notifications(token1, user1)
    test7_schedules(token1, agent1)

    print("\n" + "=" * 60)
    print("RESULTS SUMMARY")
    print("=" * 60)
    all_pass = True
    for name, result in RESULTS.items():
        status = "✅" if result == "PASS" else "❌"
        print(f"  {status} {name}: {result}")
        if result != "PASS":
            all_pass = False

    passed = sum(1 for v in RESULTS.values() if v == "PASS")
    total = len(RESULTS)
    print(f"\n  {passed}/{total} tests passed")

    if all_pass:
        print("\n🎉 ALL TESTS PASS!")
    else:
        print("\n⚠️  Some tests failed")

    sys.exit(0 if all_pass else 1)
