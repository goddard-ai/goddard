#!/usr/bin/env python3
"""Native bridge fixture: no desktop, clipboard, browser, or app access."""
import json
import os
from pathlib import Path
import sys

root = Path(os.environ["GODDARD_COMPUTER_USE_PROCESS_DIRECTORY"])
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request["method"]
    if method == "initialize":
        result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": name, "inputSchema": {"type": "object"}} for name in ["clipboard_read", "list_apps"]]}
    else:
        name = request["params"]["name"]
        with (root / "native-executions").open("a") as log:
            log.write(name + "\n")
        result = {"content": [{"type": "text", "text": "approved fixture operation"}]}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
