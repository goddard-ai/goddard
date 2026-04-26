import { expect, test } from "bun:test"

import { HttpError, type BackendControlPlane } from "../src/api/control-plane.ts"
import { createBackendRouter } from "../src/api/router.ts"
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

test("createBackendRouter delegates stream route to injected handleUserStream", async () => {
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
    createContext(
      new Request("https://example.test/remote-repo/stream", {
        headers: { authorization: "Bearer tok_1" },
      }),
    ) as any,
  )

  expect(response.status).toBe(200)
  expect(await response.text()).toBe("stream-ok")
  expect(capturedGithubUsername).toBe("alec")
})

test("createBackendRouter dispatches normalized remote-repo webhook events", async () => {
  const handled: string[] = []
  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    handleGitHubWebhook(input) {
      return {
        type: "comment",
        owner: input.owner,
        repo: input.repo,
        prNumber: input.prNumber,
        author: input.author,
        body: input.body,
        reactionAdded: "eyes",
        createdAt: "2026-01-01T00:00:00.000Z",
      }
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    broadcastEvent: async () => {},
    remoteRepoEventHandlers: [
      {
        name: "pull-request",
        handle: (event) => {
          handled.push(event.type)
        },
      },
    ],
  })

  const response = await router(
    createContext(
      new Request("https://example.test/webhooks/github", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          type: "issue_comment",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 1,
          author: "alec",
          body: "looks good",
        }),
      }),
    ) as any,
  )

  expect(response.status).toBe(200)
  expect(handled).toEqual(["comment"])
})

test("createBackendRouter delegates cloud session commands to injected handler", async () => {
  let capturedGithubUsername = ""
  let capturedPathname = ""
  let capturedSessionId = ""
  let capturedBody: unknown

  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    getSession(token) {
      expect(token).toBe("tok_1")
      return { token, githubUsername: "alec", githubUserId: 1 }
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    handleCloudSession: async (_env, githubUsername, _request, options) => {
      capturedGithubUsername = githubUsername
      capturedPathname = options.pathname
      capturedSessionId = options.sessionId
      capturedBody = options.body
      return Response.json({ accepted: true, duplicate: false, commandId: "cmd_1" })
    },
  })

  const response = await router(
    createContext(
      new Request("https://example.test/cloud/sessions/cls_cloud/commands", {
        method: "POST",
        headers: {
          authorization: "Bearer tok_1",
          "content-type": "application/json",
        },
        body: JSON.stringify({
          commandId: "cmd_1",
          type: "prompt",
          payload: { prompt: "Ship it" },
        }),
      }),
    ) as any,
  )

  expect(response.status).toBe(200)
  expect(capturedGithubUsername).toBe("alec")
  expect(capturedSessionId).toBe("cls_cloud")
  expect(capturedPathname).toBe("/commands")
  expect(capturedBody).toEqual({
    commandId: "cmd_1",
    type: "prompt",
    payload: { prompt: "Ship it" },
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
      new Request("https://example.test/auth/session/current", {
        headers: { authorization: "Bearer bad" },
      }),
    ) as any,
  )

  expect(response.status).toBe(401)
  const payload = (await response.json()) as { error: string }
  expect(payload.error).toBe("Invalid token")
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
