import { PROTOCOL_VERSION } from "../packages/waku-client/src/generated/constants";

/** A failed or ambiguous attempt never authorizes a signal-based shutdown. */
export async function requestIdleShutdown(
  address: string,
  token: string,
  ready: () => boolean,
): Promise<boolean> {
  if (!ready()) return false;
  const socket = new WebSocket(`ws://${address}/v1`);
  const prepare = crypto.randomUUID();
  const commit = crypto.randomUUID();
  let fence: string | undefined;
  return await new Promise<boolean>((resolve) => {
    let finished = false;
    const finish = (committed: boolean) => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      if (!committed && fence && socket.readyState === WebSocket.OPEN) {
        try { socket.send(JSON.stringify({ type: "releaseIdleShutdown", fence })); } catch {}
      }
      try { socket.close(); } finally { resolve(committed); }
    };
    const timer = setTimeout(() => finish(false), 4_000);
    socket.onopen = () => socket.send(JSON.stringify({
      type: "hello", protocolVersion: PROTOCOL_VERSION, token,
      clientId: crypto.randomUUID(), resumeFrom: [],
    }));
    socket.onclose = () => finish(false);
    socket.onerror = () => finish(false);
    socket.onmessage = (event) => {
      try {
        const message = JSON.parse(String(event.data));
        if (message.type === "hello") {
          if (!ready()) return finish(false);
          socket.send(JSON.stringify({ type: "prepareIdleShutdown", requestId: prepare }));
        } else if (message.type === "idleShutdown" && message.requestId === prepare) {
          if (typeof message.fence !== "string" || message.committed !== false) return finish(false);
          fence = message.fence;
          // Source edits, queued work and builds can arrive during either RPC.
          // There is no await between this check and committing this lease.
          if (!ready()) return finish(false);
          socket.send(JSON.stringify({ type: "commitIdleShutdown", requestId: commit, fence }));
        } else if (message.type === "idleShutdown" && message.requestId === commit) {
          finish(message.fence === fence && message.committed === true);
        } else if (message.type === "rejected") {
          finish(false);
        }
      } catch { finish(false); }
    };
  });
}
