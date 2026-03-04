import { DurableObject } from "cloudflare:workers";
import type { RepoEvent } from "@goddard-ai/schema";

type SseSession = {
  writer: WritableStreamDefaultWriter<Uint8Array>;
};

export class RepoStream extends DurableObject {
  #sessions = new Set<SseSession>();
  #encoder = new TextEncoder();

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/internal/broadcast" && request.method === "POST") {
      const event = await request.json();
      await this.broadcast(event as RepoEvent);
      return new Response("ok");
    }

    if (request.method !== "GET") {
      return new Response("Method Not Allowed", { status: 405 });
    }

    const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
    const writer = writable.getWriter();
    const session: SseSession = { writer };

    this.#sessions.add(session);
    await writer.write(this.#encoder.encode(": connected\n\n"));

    request.signal.addEventListener(
      "abort",
      () => {
        this.removeSession(session);
      },
      { once: true }
    );

    return new Response(readable, {
      status: 200,
      headers: {
        "content-type": "text/event-stream",
        "cache-control": "no-cache, no-transform",
        connection: "keep-alive"
      }
    });
  }

  async broadcast(event: RepoEvent): Promise<void> {
    const payload = this.#encoder.encode(formatSseDataFrame(JSON.stringify({ event })));
    await Promise.allSettled(
      [...this.#sessions].map(async (session) => {
        try {
          await session.writer.write(payload);
        } catch {
          this.removeSession(session);
        }
      })
    );
  }

  removeSession(session: SseSession): void {
    this.#sessions.delete(session);
    void session.writer.close().catch(() => {});
  }
}

function formatSseDataFrame(payload: string): string {
  const normalized = payload.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  const lines = normalized.split("\n");
  return `${lines.map((line) => `data: ${line}`).join("\n")}\n\n`;
}
