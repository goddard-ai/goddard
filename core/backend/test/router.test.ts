import { test, assert } from "vitest"
import { Webhooks } from "@octokit/webhooks"
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

test("createBackendRouter verifies and handles signed GitHub webhook deliveries", async () => {
  let broadcasted = false

  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    handleGitHubWebhook(input) {
      assert.deepEqual(input, {
        type: "issue_comment",
        owner: "goddard-ai",
        repo: "sdk",
        prNumber: 7,
        author: "teammate",
        body: "looks good",
      })

      return {
        type: "comment",
        owner: input.owner,
        repo: input.repo,
        prNumber: input.prNumber,
        author: input.author,
        body: input.body,
        reactionAdded: "eyes",
        createdAt: "2026-03-17T00:00:00.000Z",
      }
    },
  }

  const body = JSON.stringify({
    action: "created",
    issue: { number: 7, pull_request: { url: "https://example.test/pr/7" } },
    comment: {
      id: 11,
      body: "looks good",
      user: { login: "teammate", type: "User" },
    },
    repository: {
      name: "sdk",
      owner: { login: "goddard-ai" },
    },
    sender: { login: "teammate", type: "User" },
  })

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    broadcastToRepo: async (_env, owner, repo, event) => {
      broadcasted = true
      assert.equal(owner, "goddard-ai")
      assert.equal(repo, "sdk")
      assert.equal(event.type, "comment")
    },
  })

  const response = await router(
    createContext(
      new Request("https://example.test/webhooks/github", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-github-delivery": "delivery-1",
          "x-github-event": "issue_comment",
          "x-hub-signature-256": await signWebhookPayload("secret", body),
        },
        body,
      }),
      createEnv({ GITHUB_WEBHOOK_SECRET: "secret" }),
    ) as any,
  )

  assert.equal(response.status, 200)
  assert.deepEqual(await response.json(), {
    handled: true,
    event: {
      type: "comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 7,
      author: "teammate",
      body: "looks good",
      reactionAdded: "eyes",
      createdAt: "2026-03-17T00:00:00.000Z",
    },
  })
  assert.equal(broadcasted, true)
})

test("createBackendRouter rejects invalid GitHub webhook signatures", async () => {
  const router = createBackendRouter({
    createControlPlane: () => stubControlPlane,
  })

  const response = await router(
    createContext(
      new Request("https://example.test/webhooks/github", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-github-delivery": "delivery-1",
          "x-github-event": "issue_comment",
          "x-hub-signature-256": "sha256=invalid",
        },
        body: JSON.stringify({ action: "created" }),
      }),
      createEnv({ GITHUB_WEBHOOK_SECRET: "secret" }),
    ) as any,
  )

  assert.equal(response.status, 400)
  assert.deepEqual(await response.json(), {
    error: "[@octokit/webhooks] signature does not match event payload and secret",
  })
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

async function signWebhookPayload(secret: string, payload: string): Promise<string> {
  return new Webhooks({ secret }).sign(payload)
}
