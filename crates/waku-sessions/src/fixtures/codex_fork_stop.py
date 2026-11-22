#!/usr/bin/env python3
"""Hold a public native operation; the source is never mutated."""
import json
import os
from pathlib import Path
import sys
import time

root = Path(__file__).parent
phase = (root / "phase").read_text()
(root / "pid").write_text(str(os.getpid()))
(root / "original").write_text("unchanged")

def hold(point):
    if phase == point:
        (root / "held").touch()
        time.sleep(30)

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "thread/turns/list":
        hold("read")
        result = {"data": [{"id": "turn-2"}, {"id": "turn-1"}]}
    elif method == "thread/fork":
        hold("fork")
        result = {"thread": {"id": "copy"}}
    else:
        result = {}
    if "id" in request:
        print(json.dumps({"id": request["id"], "result": result}), flush=True)
hold("close")
