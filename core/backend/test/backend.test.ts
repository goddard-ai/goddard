import { createRemoteRepoBackendEvent } from "@goddard-ai/remote-repo/backend"
import { expect, test } from "bun:test"

import { InMemoryBackendControlPlane, startBackendServer } from "../src/index.ts"

test("control plane creates PR authored by authenticated user", () => {
  const backend = new InMemoryBackendControlPlane()
  const flow = backend.startDeviceFlow({ provider: "github", loginHint: "alec" })
  const session = backend.completeDeviceFlow({
    deviceCode: flow.deviceCode,
    providerIdentity: githubIdentity("alec"),
  })

  const pr = backend.createPr(session.token, {
    provider: "github",
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
    const flow = await postJson(`${baseUrl}/auth/device/start`, githubStart("alec"))
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      providerIdentity: githubIdentity("alec"),
    })

    const pr = await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        provider: "github",
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
    const flow = await postJson(`${baseUrl}/auth/device/start`, githubStart("alec"))
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      providerIdentity: githubIdentity("alec"),
    })

    await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        provider: "github",
        owner: "goddard-ai",
        repo: "test-repo",
        title: "Add CLI",
        head: "feat/cli",
        base: "main",
      },
      alecSession.token,
    )

    const managedResponse = await fetch(
      `${baseUrl}/pull-requests/managed?provider=github&owner=goddard-ai&repo=test-repo&prNumber=1`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    expect(managedResponse.status).toBe(200)
    const managedPayload = (await managedResponse.json()) as {
      managed: boolean
    }
    expect(managedPayload.managed).toBe(true)

    const unmanagedResponse = await fetch(
      `${baseUrl}/pull-requests/managed?provider=github&owner=goddard-ai&repo=test-repo&prNumber=9`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    expect(unmanagedResponse.status).toBe(200)
    const unmanagedPayload = (await unmanagedResponse.json()) as {
      managed: boolean
    }
    expect(unmanagedPayload.managed).toBe(false)

    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, githubStart("bob"))
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      providerIdentity: githubIdentity("bob"),
    })

    const foreignResponse = await fetch(
      `${baseUrl}/pull-requests/managed?provider=github&owner=goddard-ai&repo=test-repo&prNumber=1`,
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
    const flow = backend.startDeviceFlow(githubStart("alec"))
    const session = backend.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      providerIdentity: githubIdentity("alec"),
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

test("ndjson event stream receives webhook events for a managed PR", async () => {
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

    const streamResponsePromise = fetch(`${baseUrl}/events/stream`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${session.token}`,
      },
      body: JSON.stringify({ names: ["comment"] }),
    })

    await Bun.sleep(10)
    const eventPromise = streamResponsePromise.then((streamResponse) => {
      expect(streamResponse.status).toBe(200)
      return readFirstNdjsonEvent(streamResponse)
    })

    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "looks good",
    })

    const parsed = (await eventPromise) as { type: string; reactionAdded: string }
    expect(parsed.type).toBe("comment")
    expect(parsed.reactionAdded).toBe("eyes")
  } finally {
    await server.close()
  }
})

test("unified stream only emits events for managed PRs owned by the authenticated user", async () => {
  const backend = new InMemoryBackendControlPlane()
  const server = await startBackendServer(backend, {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const alecFlow = await postJson(`${baseUrl}/auth/device/start`, githubStart("alec"))
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: alecFlow.deviceCode,
      providerIdentity: githubIdentity("alec"),
    })
    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, githubStart("bob"))
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      providerIdentity: githubIdentity("bob"),
    })

    await postJson(
      `${baseUrl}/pull-requests/create`,
      {
        provider: "github",
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
        provider: "github",
        owner: "goddard-ai",
        repo: "daemon",
        title: "Bob PR",
        head: "feat/bob",
        base: "main",
      },
      bobSession.token,
    )

    const alecStreamPromise = fetch(`${baseUrl}/events/stream`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${alecSession.token}`,
      },
      body: JSON.stringify({ where: { repo: "sdk" } }),
    })
    const bobAbort = new AbortController()
    const bobStreamPromise = fetch(`${baseUrl}/events/stream`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${bobSession.token}`,
      },
      body: JSON.stringify({ where: { repo: "sdk" } }),
      signal: bobAbort.signal,
    })

    await Bun.sleep(10)
    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "looks good",
    })

    const alecEvent = (await alecStreamPromise.then(readFirstNdjsonEvent)) as { prNumber: number }
    expect(alecEvent.prNumber).toBe(1)
    await assertNoStreamResponse(bobStreamPromise, 100)
    bobAbort.abort()
    await bobStreamPromise.catch(() => {})
  } finally {
    await server.close()
  }
})

test("unified stream ignores webhook events for unmanaged PRs", async () => {
  const backend = new InMemoryBackendControlPlane()
  const server = await startBackendServer(backend, {
    port: 0,
  })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, githubStart("alec"))
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      providerIdentity: githubIdentity("alec"),
    })

    const streamAbort = new AbortController()
    const streamResponsePromise = fetch(`${baseUrl}/events/stream`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${session.token}`,
      },
      body: JSON.stringify({ names: ["comment"] }),
      signal: streamAbort.signal,
    })

    await Bun.sleep(10)
    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 99,
      author: "teammate",
      body: "looks good",
    })

    await assertNoStreamResponse(streamResponsePromise, 100)
    streamAbort.abort()
    await streamResponsePromise.catch(() => {})
  } finally {
    await server.close()
  }
})

async function readFirstNdjsonEvent(response: Response, timeoutMs = 1000): Promise<unknown> {
  if (!response.body) {
    throw new Error("Missing NDJSON response body")
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

    let separatorIndex = buffer.indexOf("\n")
    while (separatorIndex !== -1) {
      const line = buffer.slice(0, separatorIndex).trim()
      buffer = buffer.slice(separatorIndex + 1)

      if (line) {
        await reader.cancel()
        return JSON.parse(line)
      }

      separatorIndex = buffer.indexOf("\n")
    }
  }

  throw new Error("NDJSON stream ended before emitting data")
}

async function assertNoStreamResponse(
  response: Promise<Response>,
  timeoutMs: number,
): Promise<void> {
  const marker = Symbol("timeout")
  const result = await Promise.race([
    response.then(() => "resolved" as const),
    Bun.sleep(timeoutMs).then(() => marker),
  ])

  expect(result).toBe(marker)
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
          reject(new Error(`Timed out waiting for NDJSON event after ${timeoutMs}ms`))
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

function githubStart(login: string) {
  return {
    provider: "github",
    loginHint: login,
  }
}

function githubIdentity(login: string) {
  return {
    provider: "github",
    subject: String(hashTestIdentity(login)),
    displayName: login,
  }
}

function hashTestIdentity(value: string): number {
  let hash = 0
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(i)
    hash |= 0
  }
  return Math.abs(hash) + 1000
}
