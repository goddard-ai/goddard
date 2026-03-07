import test from "node:test"
import assert from "node:assert/strict"
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

  assert.equal(pr.number, 1)
  assert.match(pr.body, /Authored via CLI by @alec/)
})

test("http api supports login and pr creation", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    const pr = await postJson(
      `${baseUrl}/pr/create`,
      {
        owner: "goddard-ai",
        repo: "cmd",
        title: "Add CLI",
        head: "feat/cli",
        base: "main",
      },
      session.token,
    )

    assert.equal(pr.number, 1)
  } finally {
    await server.close()
  }
})

test("managed PR endpoint returns true only for PRs created by the authenticated user", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    // alec creates a PR
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    await postJson(
      `${baseUrl}/pr/create`,
      { owner: "goddard-ai", repo: "cmd", title: "Add CLI", head: "feat/cli", base: "main" },
      alecSession.token,
    )

    // alec sees her own PR as managed
    const managedResponse = await fetch(
      `${baseUrl}/pr/managed?owner=goddard-ai&repo=cmd&prNumber=1`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    assert.equal(managedResponse.status, 200)
    assert.deepEqual(await managedResponse.json(), { managed: true })

    // alec sees non-existent PR as unmanaged
    const unmanagedResponse = await fetch(
      `${baseUrl}/pr/managed?owner=goddard-ai&repo=cmd&prNumber=9`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    assert.equal(unmanagedResponse.status, 200)
    assert.deepEqual(await unmanagedResponse.json(), { managed: false })

    // a different user cannot see alec's PR as managed (V2 ownership check)
    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "bob" })
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      githubUsername: "bob",
    })

    const foreignResponse = await fetch(
      `${baseUrl}/pr/managed?owner=goddard-ai&repo=cmd&prNumber=1`,
      { headers: { authorization: `Bearer ${bobSession.token}` } },
    )
    assert.equal(foreignResponse.status, 200)
    assert.deepEqual(await foreignResponse.json(), { managed: false })
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

    assert.throws(() => backend.getSession(session.token), /Session expired/)
  } finally {
    Date.now = originalNow
  }
})

test("invalid JSON body returns 400", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const response = await fetch(`${baseUrl}/auth/device/start`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: "{",
    })

    assert.equal(response.status, 400)
    const payload = (await response.json()) as { message: string }
    assert.equal(payload.message, "Invalid request body")
  } finally {
    await server.close()
  }
})

test("sse stream receives webhook events", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    const streamResponse = await fetch(`${baseUrl}/stream?owner=goddard-ai&repo=sdk`, {
      headers: {
        accept: "text/event-stream",
        authorization: `Bearer ${session.token}`,
      },
    })

    assert.equal(streamResponse.status, 200)
    const eventPromise = readFirstSseEvent(streamResponse)

    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "looks good",
    })

    const parsed = (await eventPromise) as { event: { type: string; reactionAdded: string } }
    assert.equal(parsed.event.type, "comment")
    assert.equal(parsed.event.reactionAdded, "eyes")
  } finally {
    await server.close()
  }
})

async function readFirstSseEvent(response: Response): Promise<unknown> {
  if (!response.body) {
    throw new Error("Missing SSE response body")
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

async function postJson(url: string, payload: unknown, token?: string): Promise<any> {
  const response = await fetch(url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify(payload),
  })

  if (!response.ok) {
    throw new Error(`Request failed: ${response.status} ${await response.text()}`)
  }

  return response.json()
}
