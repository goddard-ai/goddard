import test from "node:test";
import assert from "node:assert/strict";
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

test("pr create and action trigger require authentication", async () => {
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

    if (url.endsWith("/actions/trigger")) {
      return jsonResponse(200, {
        id: 10,
        owner: "org",
        repo: "repo",
        workflowId: "ci",
        ref: "main",
        status: "queued",
        triggeredBy: "alec",
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

  const run = await sdk.actions.trigger({ owner: "org", repo: "repo", workflowId: "ci", ref: "main" });
  assert.equal(run.status, "queued");
});

function jsonResponse(status: number, payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      "content-type": "application/json"
    }
  });
}
