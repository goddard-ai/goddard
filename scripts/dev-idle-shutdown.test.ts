import { expect, test } from "bun:test";
import { requestIdleShutdown } from "./dev-idle-shutdown";

for (const behavior of ["commit", "busy", "build", "disconnect", "stale"] as const) {
  test(`idle shutdown public wire: ${behavior}`, async () => {
    let ready = true;
    const messages: string[] = [];
    const server = Bun.serve({
      port: 0,
      fetch(request, server) { if (server.upgrade(request)) return; return new Response(null, { status: 400 }); },
      websocket: {
        message(socket, data) {
          const message = JSON.parse(String(data));
          messages.push(message.type);
          if (message.type === "hello") socket.send(JSON.stringify({ type: "hello" }));
          if (message.type === "prepareIdleShutdown") {
            if (behavior === "disconnect") return socket.close();
            if (behavior === "build") ready = false;
            socket.send(JSON.stringify({ type: "idleShutdown", requestId: message.requestId, fence: behavior === "busy" ? null : "lease", committed: false }));
          }
          if (message.type === "commitIdleShutdown") socket.send(JSON.stringify({ type: "idleShutdown", requestId: message.requestId, fence: behavior === "stale" ? "other" : message.fence, committed: true }));
        },
      },
    });
    try {
      expect(await requestIdleShutdown(`127.0.0.1:${server.port}`, "test", () => ready)).toBe(behavior === "commit");
      if (behavior === "build" || behavior === "busy" || behavior === "disconnect") expect(messages).not.toContain("commitIdleShutdown");
    } finally { server.stop(true); }
  });
}

test("idle shutdown timeout closes its connection and a fresh attempt can succeed", async () => {
  let attempts = 0;
  let closed = 0;
  const server = Bun.serve({
    port: 0,
    fetch(request, server) { if (server.upgrade(request)) return; return new Response(null, { status: 400 }); },
    websocket: {
      close() { closed++; },
      message(socket, data) {
        const message = JSON.parse(String(data));
        if (message.type === "hello") socket.send(JSON.stringify({ type: "hello" }));
        if (message.type === "prepareIdleShutdown" && ++attempts > 1) socket.send(JSON.stringify({ type: "idleShutdown", requestId: message.requestId, fence: "fresh", committed: false }));
        if (message.type === "commitIdleShutdown") socket.send(JSON.stringify({ type: "idleShutdown", requestId: message.requestId, fence: message.fence, committed: true }));
      },
    },
  });
  try {
    expect(await requestIdleShutdown(`127.0.0.1:${server.port}`, "test", () => true)).toBe(false);
    expect(await requestIdleShutdown(`127.0.0.1:${server.port}`, "test", () => true)).toBe(true);
    expect(closed).toBeGreaterThanOrEqual(1);
  } finally { server.stop(true); }
}, 10_000);
