#!/usr/bin/env python3
"""Deterministic external ACP peer: approval is the turn's release barrier."""
import os
import json
import sys
from pathlib import Path

assert not any(key.startswith(("GODDARD_", "WAKU_")) for key in os.environ)

pending = None
prompt_count = 0
config = [
    {"id":"model", "name":"Model", "category":"model", "type":"select", "currentValue":"model-a", "options":[{"value":"model-a","name":"A"},{"value":"model-b","name":"B"},{"value":"ignored","name":"Ignored"},{"value":"no-response","name":"No response"}]},
    {"id":"thought_level", "name":"Effort", "category":"thought_level", "type":"select", "currentValue":"low", "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]},
]

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
        reply(request, {"sessionId": "original", "configOptions": config})
    elif method == "session/set_config_option":
        params = request["params"]
        Path("config-writes").open("a").write(json.dumps(params) + "\n")
        if params["value"] == "no-response":
            continue
        if params["value"] != "ignored":
            for option in config:
                if option["id"] == params["configId"]:
                    option["currentValue"] = params["value"]
        reply(request, {"configOptions": config})
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
        if request["params"]["prompt"][0]["text"] == "input":
            send({"jsonrpc":"2.0", "id":"original-input", "method":"elicitation/create", "params":{"sessionId":"original", "message":"Release label?", "requestedSchema":{"type":"object", "properties":{"label":{"type":"string"}}, "required":["label"]}}})
        elif request["params"]["prompt"][0]["text"] == "flood":
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

    elif request.get("id") == "original-input" and "result" in request and pending:
        Path("input-answer").write_text(json.dumps(request["result"]))
        reply(pending, {"stopReason": "end_turn"})
        pending = None
