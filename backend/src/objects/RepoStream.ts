import { DurableObject } from "cloudflare:workers";
import type { RepoEvent } from "@goddard-ai/shared";

export class RepoStream extends DurableObject {
  #sessions = new Set<WebSocket>();

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/internal/broadcast" && request.method === "POST") {
      const event = await request.json();
      await this.broadcast(event as any);
      return new Response("ok");
    }

    const upgradeHeader = request.headers.get("Upgrade");
    if (!upgradeHeader || upgradeHeader !== "websocket") {
      return new Response("Expected Upgrade: websocket", { status: 426 });
    }

    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];
    await this.handleSession(server);

    return new Response(null, {
      status: 101,
      webSocket: client,
    });
  }

  async handleSession(ws: WebSocket): Promise<void> {
    ws.accept();
    this.#sessions.add(ws);

    ws.addEventListener("message", async (msg) => {
      // In this version, we only broadcast FROM the server to the client.
      // But we could handle incoming messages if needed.
    });

    ws.addEventListener("close", () => {
      this.#sessions.delete(ws);
    });

    ws.addEventListener("error", () => {
      this.#sessions.delete(ws);
    });
  }

  async broadcast(event: RepoEvent): Promise<void> {
    const payload = JSON.stringify({ event });
    for (const session of this.#sessions) {
      try {
        session.send(payload);
      } catch {
        this.#sessions.delete(session);
      }
    }
  }
}
