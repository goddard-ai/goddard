#!/usr/bin/env python3
"""Local ACP peer that permits only its advertised default model."""
import json
import pathlib
import sys

trace = pathlib.Path(__file__).with_name("trace.jsonl")
config = [{"id": "model", "name": "Model", "category": "model", "type": "select",
           "currentValue": "available-model", "options": [
               {"value": "available-model", "name": "Available"}]}]
for line in sys.stdin:
    value = json.loads(line)
    if "id" not in value:
        continue
    method = value["method"]
    result, error = {}, None
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {}, "authMethods": []}
    elif method == "session/new":
        result = {"sessionId": "fixture-session", "configOptions": config}
    elif method in ("session/set_model", "session/set_config_option"):
        error = {"code": -32602, "message": "fixture does not offer the selected model"}
    elif method == "session/prompt":
        with trace.open("a") as out:
            out.write(json.dumps({"executed_model": "available-model"}) + "\n")
        result = {"stopReason": "end_turn"}
    response = {"jsonrpc": "2.0", "id": value["id"]}
    response["error" if error else "result"] = error if error else result
    print(json.dumps(response), flush=True)
