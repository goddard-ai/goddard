#!/usr/bin/env python3
"""Deterministic external ACP peer: approval is the turn's release barrier."""
import os
import json
import sys
from pathlib import Path

assert not any(key.startswith(("GODDARD_", "WAKU_")) for key in os.environ)

pending = None
prompt_count = 0

def send(message):
    print(json.dumps(message), flush=True)

def reply(request, result):
    send({"jsonrpc": "2.0", "id": request["id"], "result": result})

def update(text):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "original", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}})

for raw in sys.stdin:
    request = json.loads(raw)
    method = request.get("method")
    if method == "initialize":
        reply(request, {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}, "authMethods": []})
    elif method == "session/new":
        reply(request, {"sessionId": "original"})
    elif method == "session/prompt":
        prompt_count += 1
        Path("prompts").write_text(f"{prompt_count}\n")
        pending = request
        if request["params"]["prompt"][0]["text"] in ("refusal", "max_tokens", "max_turn_requests"):
            reply(pending, {"stopReason": request["params"]["prompt"][0]["text"]})
            pending = None
            continue
        if request["params"]["prompt"][0]["text"] == "lose-connection":
            sys.exit(1)
        if request["params"]["prompt"][0]["text"] == "flood":
            for i in range(10000):
                update(str(i))
        else:
            update("before detach")
            send({"jsonrpc": "2.0", "id": "original-permission", "method": "session/request_permission", "params": {"sessionId": "original", "toolCall": {"toolCallId": "tool-original", "title": "controlled tool"}, "options": [{"optionId": "allow", "name": "Allow once", "kind": "allow_once"}]}})
    elif method == "session/cancel":
        if pending:
            reply(pending, {"stopReason": "cancelled"})
            pending = None
    elif request.get("id") == "original-permission" and "result" in request and pending:
        if request["result"].get("outcome", {}).get("outcome") == "selected":
            reply(pending, {"stopReason": "end_turn"})
            pending = None
