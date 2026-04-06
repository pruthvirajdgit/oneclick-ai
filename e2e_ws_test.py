import asyncio, json, sys, time

async def test_chat():
    import websockets
    
    JWT = sys.argv[1]
    AGENT_ID = sys.argv[2]
    url = f"ws://localhost:8080/api/agents/{AGENT_ID}/chat?token={JWT}"
    
    print(f"[WS] Connecting...")
    messages = []
    got_done = False
    got_waking = False
    got_ready = False
    got_thinking = False
    llm_content = ""
    
    try:
        async with websockets.connect(url, ping_interval=30, ping_timeout=120, close_timeout=10) as ws:
            msg = json.dumps({"type": "message", "content": "Say hello in one word"})
            await ws.send(msg)
            print(f"[WS] Sent message")
            
            start = time.time()
            while time.time() - start < 150:
                try:
                    raw = await asyncio.wait_for(ws.recv(), timeout=10)
                    data = json.loads(raw)
                    msg_type = data.get("type", "")
                    content = data.get("content", data.get("message", ""))
                    messages.append(data)
                    
                    elapsed = time.time() - start
                    print(f"[WS] [{elapsed:.0f}s] type={msg_type} content={str(content)[:120]}")
                    
                    if "waking" in str(content).lower():
                        got_waking = True
                    if "ready" in str(content).lower():
                        got_ready = True
                    if "thinking" in str(content).lower():
                        got_thinking = True
                    if msg_type == "done":
                        got_done = True
                        llm_content = str(content)
                    if msg_type == "response" or msg_type == "assistant":
                        got_done = True
                        llm_content = str(content)
                    # If we got LLM content, we're done
                    if got_done and llm_content:
                        break
                        
                except asyncio.TimeoutError:
                    continue
                except websockets.ConnectionClosed as e:
                    print(f"[WS] Connection closed: {e}")
                    break
    except Exception as e:
        print(f"[WS] Error: {e}")
    
    result = {
        "status": "pass" if got_done and llm_content else ("partial" if got_ready else "fail"),
        "got_waking": got_waking,
        "got_ready": got_ready,
        "got_thinking": got_thinking,
        "got_done": got_done,
        "llm_content": llm_content[:200],
        "total_messages": len(messages),
        "error": "" if (got_done and llm_content) else "No LLM response received"
    }
    print(f"\n[RESULT] {json.dumps(result)}")

asyncio.run(test_chat())
