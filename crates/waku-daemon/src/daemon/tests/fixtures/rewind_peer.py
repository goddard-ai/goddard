#!/usr/bin/env python3
"""Synthetic native history: paged reads and an independently committed fork."""
import json
import os
from pathlib import Path
import sys

root = Path(__file__).parent
mode = (root / "mode").read_text()
ledger_path = root / "ledger.json"

def emit(request, result=None, error=None, code=-32000):
    value = {"id": request["id"]}
    if error is not None:
        value["error"] = {"code": code, "message": error}
    else:
        value["result"] = result or {}
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        emit(request)
    elif method in ("thread/start", "thread/resume"):
        emit(request, {"thread": {"id": "native-source", "historyMode": "paginated", "turns": []}})
    elif method == "thread/turns/list":
        if mode == "legacy":
            emit(request, error="legacy peer has no turn-page method", code=-32601)
            continue
        if mode == "read-rejected":
            emit(request, error="controlled history-read rejection")
            continue
        params = request["params"]
        offset = int(params.get("cursor") or 0)
        turn = offset + 1 if params.get("sortDirection") == "asc" else 4 - offset
        next_cursor = "1" if mode == "repeated-cursor" else str(offset + 1)
        emit(request, {"data": [{"id": "turn-" + str(turn)}], "nextCursor": next_cursor})
    elif method == "thread/read":
        emit(request, {"thread": {"id": "native-source", "turns": [
            {"id": "turn-" + str(n)} for n in range(1, 5)
        ]}})
    elif method == "thread/fork":
        assert request["params"]["threadId"] == "native-source"
        assert request["params"]["lastTurnId"] == "turn-2"
        if mode == "rejected":
            emit(request, error="controlled fork rejection")
            continue
        ledger = json.loads(ledger_path.read_text())
        ledger["children"].append(2)
        ledger_path.write_text(json.dumps(ledger))
        if mode == "lost":
            os._exit(0)
        emit(request, {"thread": {"id": "independent-history"}})
    elif method in ("thread/revert", "thread/rollback"):
        ledger = json.loads(ledger_path.read_text())
        ledger["sourceTurns"] -= 1
        ledger_path.write_text(json.dumps(ledger))
        emit(request)
    elif "id" in request:
        emit(request, error="method not found")
