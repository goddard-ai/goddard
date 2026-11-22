#!/usr/bin/env python3
"""Local Harness wire peer for configuration admission tests. No provider calls."""
import base64
import hashlib
import http.server
import json
import os
import pathlib
import sys

if "--help" in sys.argv:
    print("--no-open")
    sys.exit(0)

root = pathlib.Path(__file__).parent
model = "initial-model"
permission = "danger-full-access"


def record(value):
    with (root / "trace.jsonl").open("a") as out:
        out.write(json.dumps(value) + "\n")


class Peer(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        key = self.headers.get("Sec-WebSocket-Key")
        if not key:
            self.send_error(404)
            return
        accept = base64.b64encode(hashlib.sha1(
            (key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        # These tests use synchronous RPC replies; event links remain idle.
        while self.rfile.read(1):
            pass

    def do_POST(self):
        global model, permission
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        method, payload = request["method"], request["payload"]
        value, error = {}, None
        if method == "session.create":
            value = {"sessionId": payload["sessionId"]}
        elif method == "session.history":
            value = {"events": [], "hasMore": False, "projections": {"values": {
                "permissions": {"currentValue": permission}}}}
        elif method == "commands/list":
            value = []
        elif method == "session.models":
            value = {"current": {"provider": "local"}}
        elif method == "session.selectModel":
            requested = payload["model"]
            record({"change": requested})
            if requested == "refused-model":
                error = {"code": "invalid-model", "message": "fixture refused the selected model"}
            else:
                model = requested
        elif method == "commands/execute":
            refused = model == "partial-model"
            if not refused:
                permission = payload["args"]["line"].split()[-1]
            record({"permission": permission, "refused": refused})
            value = {"result": {"kind": "error" if refused else "success", "text": "fixture"}}
        elif method == "session.prompt":
            record({"executed_model": model, "permission": permission})
        result = {"ok": False, "error": error} if error else {"ok": True, "value": value}
        body = json.dumps({"type": "server-response", "rpcId": request["rpcId"], "result": result}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Peer)
(root / "pid").write_text(str(os.getpid()))
print("dsh web: http://127.0.0.1:%d" % server.server_port, flush=True)
server.serve_forever()
