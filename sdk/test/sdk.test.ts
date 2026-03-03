import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { createSdk, InMemoryTokenStorage } from "../src/index.ts";

test("device flow stores token and whoami uses auth header", async () => {
  const storage = new InMemoryTokenStorage();

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input);
    if (url.endsWith("/auth/device/start")) {
      return jsonResponse(200, {
        deviceCode: "dev_1",
        userCode: "ABCD-1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5
      });
    }

    if (url.endsWith("/auth/device/complete")) {
      return jsonResponse(200, {
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42
      });
    }

    if (url.endsWith("/auth/session")) {
      assert.equal(init?.headers && (init.headers as Record<string, string>).authorization, "Bearer tok_1");
      return jsonResponse(200, {
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42
      });
    }

    return jsonResponse(404, { error: "not found" });
  };

  const sdk = createSdk({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetchImpl
  });

  const start = await sdk.auth.startDeviceFlow();
  assert.equal(start.deviceCode, "dev_1");

  const session = await sdk.auth.completeDeviceFlow({ deviceCode: start.deviceCode, githubUsername: "alec" });
  assert.equal(session.githubUsername, "alec");
  assert.equal(await storage.getToken(), "tok_1");

  const me = await sdk.auth.whoami();
  assert.equal(me.githubUserId, 42);
});

test("pr create requires authentication", async () => {
  const storage = new InMemoryTokenStorage();

  const fetchImpl: typeof fetch = async (input) => {
    const url = String(input);
    if (url.endsWith("/pr/create")) {
      return jsonResponse(200, {
        id: 1,
        number: 1,
        owner: "org",
        repo: "repo",
        title: "demo",
        body: "body",
        head: "feat",
        base: "main",
        url: "https://github.com/org/repo/pull/1",
        createdBy: "alec",
        createdAt: new Date().toISOString()
      });
    }

    return jsonResponse(404, { error: "not found" });
  };

  const sdk = createSdk({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetchImpl
  });

  await assert.rejects(() =>
    sdk.pr.create({ owner: "org", repo: "repo", title: "demo", head: "feat", base: "main" })
  );

  await storage.setToken("tok_2");

  const pr = await sdk.pr.create({ owner: "org", repo: "repo", title: "demo", head: "feat", base: "main" });
  assert.equal(pr.number, 1);
});

test("stream emits error event for malformed payloads", async () => {
  const storage = new InMemoryTokenStorage();
  await storage.setToken("tok_stream");

  const sdk = createSdk({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    webSocketImpl: FakeWebSocket as unknown as typeof WebSocket
  });

  const sub = await sdk.stream.subscribeToRepo({ owner: "org", repo: "repo" });

  let errorMessage = "";
  sub.on("error", (error) => {
    errorMessage = error instanceof Error ? error.message : String(error);
  });

  FakeWebSocket.lastInstance?.emit("message", { data: "{" });
  assert.match(errorMessage, /Invalid stream payload/);
});

function jsonResponse(status: number, payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      "content-type": "application/json"
    }
  });
}

type Listener = (event?: any) => void;

class FakeWebSocket {
  static readonly OPEN = 1;
  static lastInstance: FakeWebSocket | null = null;

  readonly OPEN = FakeWebSocket.OPEN;
  readyState = FakeWebSocket.OPEN;
  #listeners = new Map<string, Set<Listener>>();

  constructor(_url: URL) {
    FakeWebSocket.lastInstance = this;

    queueMicrotask(() => {
      this.emit("open");
    });
  }

  addEventListener(eventName: string, listener: Listener): void {
    const listeners = this.#listeners.get(eventName) ?? new Set<Listener>();
    listeners.add(listener);
    this.#listeners.set(eventName, listeners);
  }

  removeEventListener(eventName: string, listener: Listener): void {
    this.#listeners.get(eventName)?.delete(listener);
  }

  close(): void {
    this.readyState = 3;
    this.emit("close");
  }

  emit(eventName: string, payload?: any): void {
    this.#listeners.get(eventName)?.forEach((listener) => listener(payload));
  }
}

test("agents.appendSpecInstructions creates AGENTS.md with correct instructions", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-test-"));

  try {
    const sdk = createSdk({
      baseUrl: "http://127.0.0.1:8787",
      tokenStorage: new InMemoryTokenStorage()
    });

    const agentsPath = await sdk.agents.appendSpecInstructions(tempDir);
    assert.equal(agentsPath, path.join(tempDir, "AGENTS.md"));

    const content = await fs.readFile(agentsPath, "utf-8");
    assert.match(content, /The `spec` Folder/);
    assert.match(content, /domain routing hub/);
  } finally {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});
