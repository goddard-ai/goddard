import { REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED } from "@goddard-ai/remote-repo/backend"
import { expect, test } from "bun:test"

import { HttpError, type BackendControlPlane } from "../src/api/control-plane.ts"
import { authorizeBackendEventPublication, createBackendRouter } from "../src/api/router.ts"
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
}

test("createBackendRouter handles auth device start via rouzer route map", async () => {
  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    startDeviceFlow(input) {
      expect(input?.loginHint).toBe("alec")
      return {
        deviceCode: "dev_1",
        userCode: "ABCD1234",
        verificationUri: "https://auth.local/github/device",
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
        body: JSON.stringify(githubStart("alec")),
      }),
    ) as any,
  )

  expect(response.status).toBe(200)
  const payload = (await response.json()) as { deviceCode: string }
  expect(payload.deviceCode).toBe("dev_1")
})

test("createBackendRouter delegates event stream route to injected handleUserEvents", async () => {
  let capturedStreamKey = ""
  let capturedFilter: unknown

  const controlPlane: BackendControlPlane = {
    ...stubControlPlane,
    getPrincipal(token) {
      expect(token).toBe("tok_1")
      return {
        ...githubPrincipal("alec"),
        repositories: [],
      }
    },
  }

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    handleUserEvents: (_env, streamKey, filter, _request) => {
      capturedStreamKey = streamKey
      capturedFilter = filter
      return (async function* () {
        yield {
          type: "comment" as const,
          provider: "github",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 1,
          author: "teammate",
          body: "looks good",
          reactionAdded: "eyes" as const,
          createdAt: "2026-01-01T00:00:00.000Z",
        }
      })()
    },
  })

  const streamResponse = await router(
    createContext(
      new Request("https://example.test/events/stream", {
        method: "POST",
        headers: {
          authorization: "Bearer tok_1",
          "content-type": "application/json",
        },
        body: JSON.stringify({ names: ["comment"] }),
      }),
    ) as any,
  )

  expect(streamResponse.status).toBe(200)
  expect(await readFirstNdjsonLine(streamResponse)).toMatchObject({ type: "comment" })
  expect(capturedStreamKey).toBe("github:alec")
  expect(capturedFilter).toEqual({ names: ["comment"] })
})

test("createBackendRouter publishes remote-repo events from the composed GitHub route", async () => {
  const publications: unknown[] = []

  const router = createBackendRouter({
    createControlPlane: () => stubControlPlane,
    broadcastEvent: async (_env, publication) => {
      publications.push(publication)
    },
  })

  const response = await router(
    createContext(
      new Request("https://example.test/webhooks/github", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-github-event": "issue_comment",
          "x-github-delivery": "delivery-1",
        },
        body: JSON.stringify({
          action: "created",
          issue: {
            number: 1,
            pull_request: {},
          },
          comment: {
            user: { login: "alec", type: "User" },
            body: "looks good",
          },
          repository: {
            name: "sdk",
            owner: { login: "goddard-ai" },
          },
          sender: { login: "alec", type: "User" },
        }),
      }),
    ) as any,
  )

  expect(response.status).toBe(200)
  expect(publications).toMatchObject([
    {
      source: "remote-repo",
      event: {
        name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
        payload: {
          type: "comment",
          provider: "github",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 1,
        },
      },
    },
  ])
})

test("authorizeBackendEventPublication enforces source-owned repository access", async () => {
  const publication = {
    source: "remote-repo",
    event: {
      name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
      payload: {
        type: "comment",
        provider: "github",
        owner: "goddard-ai",
        repo: "sdk",
        prNumber: 1,
        author: "alec",
        body: "looks good",
        reactionAdded: "eyes",
        createdAt: "2026-01-01T00:00:00.000Z",
      },
    },
  } as const

  await expect(
    authorizeBackendEventPublication(
      {
        ...githubPrincipal("alec"),
        repositories: [{ provider: "github", owner: "goddard-ai", repo: "sdk" }],
      },
      publication,
    ),
  ).resolves.toBe(true)
  await expect(
    authorizeBackendEventPublication(
      {
        ...githubPrincipal("bob"),
        repositories: [{ provider: "github", owner: "goddard-ai", repo: "other" }],
      },
      publication,
    ),
  ).resolves.toBe(false)
})

test("authorizeBackendEventPublication rejects invalid source and event pairs", async () => {
  const principal = {
    ...githubPrincipal("alec"),
    repositories: [{ provider: "github", owner: "goddard-ai", repo: "sdk" }],
  }
  const event = {
    name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
    payload: {
      type: "comment",
      provider: "github",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "alec",
      body: "looks good",
      reactionAdded: "eyes",
      createdAt: "2026-01-01T00:00:00.000Z",
    },
  } as const

  await expect(
    authorizeBackendEventPublication(principal, {
      source: "unknown",
      event,
    } as never),
  ).rejects.toThrow("Unknown backend event source: unknown")
  await expect(
    authorizeBackendEventPublication(principal, {
      source: "remote-repo",
      event: {
        ...event,
        name: "pull_request.feedback.received",
      },
    } as never),
  ).rejects.toThrow(
    "Backend event source remote-repo cannot produce event: pull_request.feedback.received",
  )
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

function githubStart(loginHint: string) {
  return {
    provider: "github",
    loginHint,
  }
}

function githubPrincipal(subject: string) {
  return {
    id: `github:${subject}`,
    providerIdentities: [
      {
        provider: "github",
        subject,
        displayName: subject,
      },
    ],
  }
}

async function readFirstNdjsonLine(response: Response): Promise<unknown> {
  if (!response.body) {
    throw new Error("Missing NDJSON response body")
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ""

  while (true) {
    const { value, done } = await reader.read()
    if (done) {
      break
    }

    buffer += decoder.decode(value, { stream: true })
    const newline = buffer.indexOf("\n")
    if (newline !== -1) {
      await reader.cancel()
      return JSON.parse(buffer.slice(0, newline))
    }
  }

  throw new Error("NDJSON stream ended before emitting data")
}
