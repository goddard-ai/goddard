import { test, assert } from "vitest"
import { createBackendRouter } from "../src/router.ts"
import { HttpError, type BackendControlPlane } from "../src/control-plane.ts"
import type { Env } from "../src/env.ts"

const notUsed = () => {
  throw new Error("not used")
}

const stubControlPlane: BackendControlPlane = {
  startDeviceFlow: notUsed,
  completeDeviceFlow: notUsed,
  getSession: notUsed,
  createPr: notUsed,
  isManagedPr: notUsed,
  replyToPr: notUsed,
  handleGitHubWebhook: notUsed,
}

test("createBackendRouter handles auth device start via rouzer route map", async () => {
  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    startDeviceFlow(input) {
      assert.equal(input?.githubUsername, "alec")
      return {
        deviceCode: "dev_1",
        userCode: "ABCD1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      }
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
  })

  const response = await router(
    createContext(
      new Request("https://example.test/auth/device/start", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ githubUsername: "alec" }),
      }),
    ) as any,
  )

  assert.equal(response.status, 200)
  const payload = (await response.json()) as { deviceCode: string }
  assert.equal(payload.deviceCode, "dev_1")
})

test("createBackendRouter delegates stream route to injected handleRepoStream", async () => {
  let capturedOwner = ""
  let capturedRepo = ""

  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    getSession(token) {
      assert.equal(token, "tok_1")
      return { token, githubUsername: "alec", githubUserId: 1 }
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    handleRepoStream: async (_env, owner, repo, _request) => {
      capturedOwner = owner
      capturedRepo = repo
      return new Response("stream-ok", { status: 200 })
    },
  })

  const response = await router(
    createContext(
      new Request("https://example.test/stream?owner=goddard-ai&repo=sdk", {
        headers: { authorization: "Bearer tok_1" },
      }),
    ) as any,
  )

  assert.equal(response.status, 200)
  assert.equal(await response.text(), "stream-ok")
  assert.equal(capturedOwner, "goddard-ai")
  assert.equal(capturedRepo, "sdk")
})

test("createBackendRouter serializes HttpError responses", async () => {
  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    getSession() {
      throw new HttpError(401, "Invalid token")
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
  })

  const response = await router(
    createContext(
      new Request("https://example.test/auth/session", {
        headers: { authorization: "Bearer bad" },
      }),
    ) as any,
  )

  assert.equal(response.status, 401)
  assert.deepEqual(await response.json(), { error: "Invalid token" })
})

function createContext(request: Request, env = createEnv()) {
  return {
    request,
    ip: "127.0.0.1",
    platform: { env },
    env(key: string) {
      return env[key as keyof Env] as unknown
    },
    passThrough() {},
    waitUntil() {},
  }
}

function createEnv(overrides: Partial<Env> = {}): Env {
  return {
    TURSO_DB_URL: "libsql://test",
    TURSO_DB_AUTH_TOKEN: "token",
    ...overrides,
  }
}
