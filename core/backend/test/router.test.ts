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
  getPrincipal: notUsed,
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
  let capturedGithubLogin = ""

  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    getPrincipal(token) {
      expect(token).toBe("tok_1")
      return {
        kind: "github_user",
        githubLogin: "alec",
        githubUserId: 1,
        repositories: [],
      }
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    handleUserStream: async (_env, principal, _request) => {
      capturedGithubLogin = principal.githubLogin
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
  expect(capturedGithubLogin).toBe("alec")
})

test("createBackendRouter dispatches normalized remote-repo webhook events", async () => {
  const handled: string[] = []
  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    handleGitHubWebhook(delivery) {
      return {
        name: "remote_repo.event.received",
        payload: {
          type: "comment",
          owner: delivery.event.owner,
          repo: delivery.event.repo,
          prNumber: delivery.event.prNumber,
          author: delivery.event.author,
          body: delivery.event.body,
          reactionAdded: "eyes",
          createdAt: "2026-01-01T00:00:00.000Z",
        },
        provenance: {
          provider: "github",
          deliveryId: delivery.deliveryId,
          webhookType: delivery.event.type,
        },
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
          deliveryId: "delivery-1",
          event: {
            type: "issue_comment",
            owner: "goddard-ai",
            repo: "sdk",
            prNumber: 1,
            author: "alec",
            body: "looks good",
          },
        }),
      }),
    ) as any,
  )

  expect(response.status).toBe(200)
  expect(handled).toEqual(["comment"])
})

test("createBackendRouter rejects GitHub webhooks with invalid configured signatures", async () => {
  const router = createBackendRouter({
    createControlPlane: () => stubControlPlane,
  })

  const response = await router(
    createContext(
      new Request("https://example.test/webhooks/github", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-hub-signature-256": "sha256=bad",
        },
        body: JSON.stringify({
          deliveryId: "delivery-1",
          event: {
            type: "issue_comment",
            owner: "goddard-ai",
            repo: "sdk",
            prNumber: 1,
            author: "alec",
            body: "looks good",
          },
        }),
      }),
      createEnv({ GITHUB_WEBHOOK_SECRET: "secret" }),
    ) as any,
  )

  expect(response.status).toBe(401)
  const payload = (await response.json()) as { error: string }
  expect(payload.error).toBe("Invalid GitHub webhook signature")
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
