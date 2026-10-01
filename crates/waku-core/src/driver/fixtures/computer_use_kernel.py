#!/usr/bin/env python3
"""Private transport fixture; actual JavaScript semantics use the packaged-engine test."""
import base64
import json
import os
from pathlib import Path
import sys
import time

root = Path(os.environ["GODDARD_COMPUTER_USE_PROCESS_DIRECTORY"])
calls = 0
for line in sys.stdin:
    request = json.loads(line)
    args = request["params"]["arguments"]
    code = args.get("code", "")
    with (root / "executions").open("a") as log:
        log.write(code + "\n")
    if code == "disconnect":
        sys.exit(0)
    if code == "block":
        (root / "started").touch()
        while not (root / "cancel-kernel").exists():
            time.sleep(0.01)
    if request["params"]["name"] == "js_reset":
        calls = 0
    else:
        calls += 1
    content = [{"type": "text", "text": str(calls)}]
    if code == "image":
        content.append({"type": "image", "data": base64.b64encode(b"synthetic-image-bytes").decode(), "mimeType": "image/png"})
    result = {"content": content, "isError": code in ("error", "block"), "_meta": {"pid": os.getpid(), "cwd": os.getcwd()}}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
