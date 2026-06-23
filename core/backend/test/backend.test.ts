import {
  REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
  REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
} from "@goddard-ai/remote-repo/backend"
import { expect, test } from "bun:test"

import { InMemoryBackendControlPlane, startBackendServer } from "../src/index.ts"

test("control plane creates PR authored by authenticated user", () => {
  const backend = new InMemoryBackendControlPlane()
  const flow = backend.startDeviceFlow({ githubUsername: "alec" })
  const session = backend.completeDeviceFlow({
    deviceCode: flow.deviceCode,
    githubUsername: "alec",
  })

  const pr = backend.createPr(session.token, {
    owner: "goddard-ai",
    repo: "sdk",
    title: "Fix parser",
    body: "This improves parsing",
    head: "fix/parser",
    base: "main",
  })

  expect(pr.number).toBe(1)
  expect(pr.body).toMatch(/Authored via CLI by @alec/)
})

test("http api supports login and pr creation", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "alec",
    })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    const pr = await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        owner: "goddard-ai",
        repo: "test-repo",
        title: "Add CLI",
        head: "feat/cli",
        base: "main",
      },
      session.token,
    )

    expect(pr.number).toBe(1)
  } finally {
    await server.close()
  }
})

test("managed PR endpoint returns true only for PRs created by the authenticated user", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "alec",
    })
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        owner: "goddard-ai",
        repo: "test-repo",
        title: "Add CLI",
        head: "feat/cli",
        base: "main",
      },
      alecSession.token,
    )

    const managedResponse = await fetch(
      `${baseUrl}/pull-requests/managed?owner=goddard-ai&repo=test-repo&prNumber=1`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    expect(managedResponse.status).toBe(200)
    const managedPayload = (await managedResponse.json()) as {
      managed: boolean
    }
    expect(managedPayload.managed).toBe(true)

    const unmanagedResponse = await fetch(
      `${baseUrl}/pull-requests/managed?owner=goddard-ai&repo=test-repo&prNumber=9`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    expect(unmanagedResponse.status).toBe(200)
    const unmanagedPayload = (await unmanagedResponse.json()) as {
      managed: boolean
    }
    expect(unmanagedPayload.managed).toBe(false)

    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "bob",
    })
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      githubUsername: "bob",
    })

    const foreignResponse = await fetch(
      `${baseUrl}/pull-requests/managed?owner=goddard-ai&repo=test-repo&prNumber=1`,
      { headers: { authorization: `Bearer ${bobSession.token}` } },
    )
    expect(foreignResponse.status).toBe(200)
    const foreignPayload = (await foreignResponse.json()) as {
      managed: boolean
    }
    expect(foreignPayload.managed).toBe(false)
  } finally {
    await server.close()
  }
})

test("expired auth sessions are rejected", () => {
  const originalNow = Date.now

  try {
    Date.now = () => 1000
    const backend = new InMemoryBackendControlPlane()
    const flow = backend.startDeviceFlow({ githubUsername: "alec" })
    const session = backend.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    Date.now = () => 1000 + 1000 * 60 * 60 * 24 + 1

    expect(() => backend.getSession(session.token)).toThrow(/Session expired/)
  } finally {
    Date.now = originalNow
  }
})

test("invalid JSON body returns 400", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const response = await fetch(`${baseUrl}/auth/device/start`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: "{",
    })

    expect(response.status).toBe(400)
    const payload = (await response.json()) as { message: string }
    expect(payload.message).toBe("Invalid request body")
  } finally {
    await server.close()
  }
})

test("sse stream receives webhook events for a managed PR", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "alec",
    })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        owner: "goddard-ai",
        repo: "sdk",
        title: "Add CLI",
        head: "feat/cli",
        base: "main",
      },
      session.token,
    )

    const streamResponse = await fetch(`${baseUrl}/remote-repo/stream`, {
      headers: {
        accept: "text/event-stream",
        authorization: `Bearer ${session.token}`,
      },
    })

    expect(streamResponse.status).toBe(200)
    const eventPromise = readFirstSseEvent(streamResponse)

    await postGitHubWebhook(baseUrl, "delivery-1", {
      action: "created",
      issue: {
        number: 1,
        pull_request: {},
      },
      comment: {
        user: { login: "teammate", type: "User" },
        body: "looks good",
      },
      repository: {
        name: "sdk",
        owner: { login: "goddard-ai" },
      },
      sender: { login: "teammate", type: "User" },
    })

    const parsed = (await eventPromise) as {
      name: string
      payload: { type: string; reactionAdded: string }
    }
    expect(parsed.name).toBe(REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED)
    expect(parsed.payload.type).toBe("comment")
    expect(parsed.payload.reactionAdded).toBe("eyes")
  } finally {
    await server.close()
  }
})

test("sse stream receives pull request review webhook events for a managed PR", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "alec",
    })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        owner: "goddard-ai",
        repo: "sdk",
        title: "Add CLI",
        head: "feat/cli",
        base: "main",
      },
      session.token,
    )

    const streamResponse = await fetch(`${baseUrl}/remote-repo/stream`, {
      headers: {
        accept: "text/event-stream",
        authorization: `Bearer ${session.token}`,
      },
    })

    const eventPromise = readFirstSseEvent(streamResponse)
    await postGitHubWebhook(
      baseUrl,
      "delivery-1",
      {
        action: "submitted",
        pull_request: {
          number: 1,
        },
        review: {
          user: { login: "reviewer", type: "User" },
          state: "approved",
          body: "ship it",
        },
        repository: {
          name: "sdk",
          owner: { login: "goddard-ai" },
        },
        sender: { login: "reviewer", type: "User" },
      },
      "pull_request_review",
    )

    const parsed = (await eventPromise) as {
      name: string
      payload: { type: string; state: string; reactionAdded: string }
    }
    expect(parsed.name).toBe(REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED)
    expect(parsed.payload.type).toBe("review")
    expect(parsed.payload.state).toBe("approved")
    expect(parsed.payload.reactionAdded).toBe("eyes")
  } finally {
    await server.close()
  }
})

test("unified stream only emits events for managed PRs owned by the authenticated user", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const alecFlow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "alec",
    })
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: alecFlow.deviceCode,
      githubUsername: "alec",
    })
    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "bob",
    })
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      githubUsername: "bob",
    })

    await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        owner: "goddard-ai",
        repo: "sdk",
        title: "Alec PR",
        head: "feat/alec",
        base: "main",
      },
      alecSession.token,
    )
    await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        owner: "goddard-ai",
        repo: "daemon",
        title: "Bob PR",
        head: "feat/bob",
        base: "main",
      },
      bobSession.token,
    )

    const alecStream = await fetch(`${baseUrl}/remote-repo/stream`, {
      headers: {
        accept: "text/event-stream",
        authorization: `Bearer ${alecSession.token}`,
      },
    })
    const bobStream = await fetch(`${baseUrl}/remote-repo/stream`, {
      headers: {
        accept: "text/event-stream",
        authorization: `Bearer ${bobSession.token}`,
      },
    })

    await postGitHubWebhook(baseUrl, "delivery-1", {
      action: "created",
      issue: {
        number: 1,
        pull_request: {},
      },
      comment: {
        user: { login: "teammate", type: "User" },
        body: "looks good",
      },
      repository: {
        name: "sdk",
        owner: { login: "goddard-ai" },
      },
      sender: { login: "teammate", type: "User" },
    })

    const alecEvent = (await readFirstSseEvent(alecStream)) as {
      payload: { prNumber: number }
    }
    expect(alecEvent.payload.prNumber).toBe(1)
    await assertNoSseEvent(bobStream, 100)
  } finally {
    await server.close()
  }
})

test("unified stream ignores webhook events for unmanaged PRs", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, {
      githubUsername: "alec",
    })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    const streamResponse = await fetch(`${baseUrl}/remote-repo/stream`, {
      headers: {
        accept: "text/event-stream",
        authorization: `Bearer ${session.token}`,
      },
    })

    await postGitHubWebhook(baseUrl, "delivery-1", {
      action: "created",
      issue: {
        number: 99,
        pull_request: {},
      },
      comment: {
        user: { login: "teammate", type: "User" },
        body: "looks good",
      },
      repository: {
        name: "sdk",
        owner: { login: "goddard-ai" },
      },
      sender: { login: "teammate", type: "User" },
    })

    await assertNoSseEvent(streamResponse, 100)
  } finally {
    await server.close()
  }
})

async function readFirstSseEvent(response: Response, timeoutMs = 1000): Promise<unknown> {
  if (!response.body) {
    throw new Error("Missing SSE response body")
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ""

  while (true) {
    const { value, done } = await readWithTimeout(reader, timeoutMs)
    if (done) {
      break
    }

    buffer += decoder.decode(value, { stream: true })

    let separatorIndex = buffer.indexOf("\n\n")
    while (separatorIndex !== -1) {
      const rawEvent = buffer.slice(0, separatorIndex)
      buffer = buffer.slice(separatorIndex + 2)

      const dataLines = rawEvent
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice("data:".length).trimStart())

      if (dataLines.length > 0) {
        await reader.cancel()
        return JSON.parse(dataLines.join("\n"))
      }

      separatorIndex = buffer.indexOf("\n\n")
    }
  }

  throw new Error("SSE stream ended before emitting data")
}

async function assertNoSseEvent(response: Response, timeoutMs: number): Promise<void> {
  try {
    await readFirstSseEvent(response, timeoutMs)
  } catch (error) {
    expect(String(error)).toMatch(
      /(Timed out waiting for SSE event|SSE stream ended before emitting data)/,
    )
    return
  }

  throw new Error(`Expected no SSE event within ${timeoutMs}ms`)
}

async function readWithTimeout(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  timeoutMs: number,
): Promise<ReadableStreamReadResult<Uint8Array>> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined
  const readPromise = reader.read() as Promise<ReadableStreamReadResult<Uint8Array>>

  try {
    return await Promise.race([
      readPromise,
      new Promise<ReadableStreamReadResult<Uint8Array>>((_, reject) => {
        timeoutId = setTimeout(() => {
          void reader.cancel().catch(() => {})
          reject(new Error(`Timed out waiting for SSE event after ${timeoutMs}ms`))
        }, timeoutMs)
      }),
    ])
  } finally {
    if (timeoutId) {
      clearTimeout(timeoutId)
    }
  }
}

async function postJson(
  url: string,
  payload: unknown,
  token?: string,
  headers: Record<string, string> = {},
): Promise<any> {
  const response = await fetch(url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      ...headers,
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify(payload),
  })

  if (!response.ok) {
    throw new Error(`Request failed: ${response.status} ${await response.text()}`)
  }

  return response.json()
}

async function postGitHubWebhook(
  baseUrl: string,
  deliveryId: string,
  payload: unknown,
  eventName = "issue_comment",
) {
  return postJson(`${baseUrl}/webhooks/github`, payload, undefined, {
    "x-github-event": eventName,
    "x-github-delivery": deliveryId,
  })
}
