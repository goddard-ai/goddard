import { expect, test } from "vitest"
import { createBackendRouter } from "../src/api/router.ts"
import { HttpError, type BackendControlPlane } from "../src/api/control-plane.ts"
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
  recordRepoEvent: notUsed,
  getRepoEventHistory: notUsed,
  resolveEventOwner: notUsed,
}

test("createBackendRouter handles auth device start via rouzer route map", async () => {
  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    startDeviceFlow(input) {
      expect(input?.githubUsername).toBe("alec")
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

  expect(response.status).toBe(200)
  const payload = (await response.json()) as { deviceCode: string }
  expect(payload.deviceCode).toBe("dev_1")
})

test("createBackendRouter delegates stream route to injected handleUserStream with query-token auth", async () => {
  let capturedGithubUsername = ""

  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    getSession(token) {
      expect(token).toBe("tok_1")
      return { token, githubUsername: "alec", githubUserId: 1 }
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    handleUserStream: async (_env, githubUsername, _request) => {
      capturedGithubUsername = githubUsername
      return new Response("stream-ok", { status: 200 })
    },
  })

  const response = await router(
    createContext(new Request("https://example.test/stream?token=tok_1")) as any,
  )

  expect(response.status).toBe(200)
  expect(await response.text()).toBe("stream-ok")
  expect(capturedGithubUsername).toBe("alec")
})

test("createBackendRouter delegates stream history route to the control plane", async () => {
  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    getRepoEventHistory(token, after) {
      expect(token).toBe("tok_1")
      expect(after).toBe(7)
      return [
        {
          id: 8,
          createdAt: "2026-03-21T00:00:00.000Z",
          event: {
            type: "comment",
            owner: "goddard-ai",
            repo: "sdk",
            prNumber: 1,
            author: "teammate",
            body: "ship it",
            reactionAdded: "eyes",
            createdAt: "2026-03-21T00:00:00.000Z",
          },
        },
      ]
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
  })

  const response = await router(
    createContext(
      new Request("https://example.test/stream/history?after=7", {
        headers: { authorization: "Bearer tok_1" },
      }),
    ) as any,
  )

  expect(response.status).toBe(200)
  expect(await response.json()).toEqual({
    events: [
      {
        id: 8,
        createdAt: "2026-03-21T00:00:00.000Z",
        event: {
          type: "comment",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 1,
          author: "teammate",
          body: "ship it",
          reactionAdded: "eyes",
          createdAt: "2026-03-21T00:00:00.000Z",
        },
      },
    ],
  })
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

  expect(response.status).toBe(401)
  expect(await response.json()).toEqual({ error: "Invalid token" })
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
