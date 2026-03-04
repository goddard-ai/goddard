import test from "node:test";
import assert from "node:assert/strict";
import { createBackendRouter } from "../src/router.ts";
import { HttpError, type BackendControlPlane } from "../src/control-plane.ts";
import type { Env } from "../src/env.ts";

test("createBackendRouter handles auth device start via rouzer route map", async () => {
  const controlPlane: BackendControlPlane = {
    startDeviceFlow(input) {
      assert.equal(input?.githubUsername, "alec");
      return {
        deviceCode: "dev_1",
        userCode: "ABCD1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5
      };
    },
    completeDeviceFlow() {
      throw new Error("not used");
    },
    getSession() {
      throw new Error("not used");
    },
    createPr() {
      throw new Error("not used");
    },
    handleGitHubWebhook() {
      throw new Error("not used");
    }
  };

  const router = createBackendRouter({
    createControlPlane: () => controlPlane
  });

  const response = await router(
    createContext(
      new Request("https://example.test/auth/device/start", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ githubUsername: "alec" })
      })
    ) as any
  );

  assert.equal(response.status, 200);
  const payload = (await response.json()) as { deviceCode: string };
  assert.equal(payload.deviceCode, "dev_1");
});

test("createBackendRouter proxies stream route to durable object", async () => {
  let requestedRepo = "";
  const env = createEnv({
    REPO_STREAM: {
      idFromName(name: string) {
        requestedRepo = name;
        return { toString: () => name } as DurableObjectId;
      },
      get() {
        return {
          fetch: async () => new Response("stream-ok", { status: 200 })
        } as unknown as DurableObjectStub;
      }
    } as unknown as DurableObjectNamespace
  });

  const controlPlane: BackendControlPlane = {
    startDeviceFlow() {
      throw new Error("not used");
    },
    completeDeviceFlow() {
      throw new Error("not used");
    },
    getSession(token) {
      assert.equal(token, "tok_1");
      return { token, githubUsername: "alec", githubUserId: 1 };
    },
    createPr() {
      throw new Error("not used");
    },
    handleGitHubWebhook() {
      throw new Error("not used");
    }
  };

  const router = createBackendRouter({
    createControlPlane: () => controlPlane
  });

  const response = await router(
    createContext(new Request("https://example.test/stream?owner=goddard-ai&repo=sdk&token=tok_1"), env) as any
  );

  assert.equal(response.status, 200);
  assert.equal(await response.text(), "stream-ok");
  assert.equal(requestedRepo, "goddard-ai/sdk");
});

test("createBackendRouter serializes HttpError responses", async () => {
  const controlPlane: BackendControlPlane = {
    startDeviceFlow() {
      throw new Error("not used");
    },
    completeDeviceFlow() {
      throw new Error("not used");
    },
    getSession() {
      throw new HttpError(401, "Invalid token");
    },
    createPr() {
      throw new Error("not used");
    },
    handleGitHubWebhook() {
      throw new Error("not used");
    }
  };

  const router = createBackendRouter({
    createControlPlane: () => controlPlane
  });

  const response = await router(
    createContext(
      new Request("https://example.test/auth/session", {
        headers: { authorization: "Bearer bad" }
      })
    ) as any
  );

  assert.equal(response.status, 401);
  assert.deepEqual(await response.json(), { error: "Invalid token" });
});

function createContext(request: Request, env = createEnv()) {
  return {
    request,
    ip: "127.0.0.1",
    platform: { env },
    env(key: string) {
      return env[key as keyof Env] as unknown;
    },
    passThrough() {},
    waitUntil() {}
  };
}

function createEnv(overrides: Partial<Env> = {}): Env {
  return {
    TURSO_DB_URL: "libsql://test",
    TURSO_DB_AUTH_TOKEN: "token",
    REPO_STREAM: {
      idFromName(name: string) {
        return { toString: () => name } as DurableObjectId;
      },
      get() {
        return {
          fetch: async () => new Response("ok")
        } as unknown as DurableObjectStub;
      }
    } as unknown as DurableObjectNamespace,
    ...overrides
  };
}
