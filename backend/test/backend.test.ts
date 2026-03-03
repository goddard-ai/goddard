import test from "node:test";
import assert from "node:assert/strict";
import { once } from "node:events";
import { InMemoryBackendControlPlane, startBackendServer } from "../src/index.ts";
import { WebSocket } from "ws";

test("control plane creates PR authored by authenticated user", () => {
  const backend = new InMemoryBackendControlPlane();
  const flow = backend.startDeviceFlow({ githubUsername: "alec" });
  const session = backend.completeDeviceFlow({ deviceCode: flow.deviceCode, githubUsername: "alec" });

  const pr = backend.createPr(session.token, {
    owner: "goddard-ai",
    repo: "sdk",
    title: "Fix parser",
    body: "This improves parsing",
    head: "fix/parser",
    base: "main"
  });

  assert.equal(pr.number, 1);
  assert.match(pr.body, /Authored via CLI by @alec/);
});

test("http api supports login and pr creation", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 });
  const baseUrl = `http://127.0.0.1:${server.port}`;

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" });
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec"
    });

    const pr = await postJson(
      `${baseUrl}/pr/create`,
      {
        owner: "goddard-ai",
        repo: "cmd",
        title: "Add CLI",
        head: "feat/cli",
        base: "main"
      },
      session.token
    );

    assert.equal(pr.number, 1);
  } finally {
    await server.close();
  }
});

test("expired auth sessions are rejected", () => {
  const originalNow = Date.now;

  try {
    Date.now = () => 1000;
    const backend = new InMemoryBackendControlPlane();
    const flow = backend.startDeviceFlow({ githubUsername: "alec" });
    const session = backend.completeDeviceFlow({ deviceCode: flow.deviceCode, githubUsername: "alec" });

    Date.now = () => 1000 + 1000 * 60 * 60 * 24 + 1;

    assert.throws(() => backend.getSession(session.token), /Session expired/);
  } finally {
    Date.now = originalNow;
  }
});

test("invalid JSON body returns 400", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 });
  const baseUrl = `http://127.0.0.1:${server.port}`;

  try {
    const response = await fetch(`${baseUrl}/auth/device/start`, {
      method: "POST",
      headers: {
        "content-type": "application/json"
      },
      body: "{"
    });

    assert.equal(response.status, 400);
    const payload = (await response.json()) as { error: string };
    assert.equal(payload.error, "Invalid JSON body");
  } finally {
    await server.close();
  }
});

test("websocket stream receives webhook events", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 });
  const baseUrl = `http://127.0.0.1:${server.port}`;

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" });
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec"
    });

    const ws = new WebSocket(
      `ws://127.0.0.1:${server.port}/stream?owner=goddard-ai&repo=sdk&token=${session.token}`
    );
    await once(ws, "open");

    const messagePromise = once(ws, "message");

    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "looks good"
    });

    const [payload] = await messagePromise;
    const parsed = JSON.parse(String(payload)) as { event: { type: string; reactionAdded: string } };

    assert.equal(parsed.event.type, "comment");
    assert.equal(parsed.event.reactionAdded, "eyes");

    ws.close();
  } finally {
    await server.close();
  }
});

async function postJson(url: string, payload: unknown, token?: string): Promise<any> {
  const response = await fetch(url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      ...(token ? { authorization: `Bearer ${token}` } : {})
    },
    body: JSON.stringify(payload)
  });

  if (!response.ok) {
    throw new Error(`Request failed: ${response.status} ${await response.text()}`);
  }

  return response.json();
}
