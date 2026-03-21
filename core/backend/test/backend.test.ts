import { expect, test } from "vitest"
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
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    await postJson(
      `${baseUrl}/pr/create`,
      { owner: "goddard-ai", repo: "test-repo", title: "Add CLI", head: "feat/cli", base: "main" },
      alecSession.token,
    )

    const managedResponse = await fetch(
      `${baseUrl}/pr/managed?owner=goddard-ai&repo=test-repo&prNumber=1`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    expect(managedResponse.status).toBe(200)
    expect(await managedResponse.json()).toEqual({ managed: true })

    const unmanagedResponse = await fetch(
      `${baseUrl}/pr/managed?owner=goddard-ai&repo=test-repo&prNumber=9`,
      { headers: { authorization: `Bearer ${alecSession.token}` } },
    )
    expect(unmanagedResponse.status).toBe(200)
    expect(await unmanagedResponse.json()).toEqual({ managed: false })

    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "bob" })
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      githubUsername: "bob",
    })

    const foreignResponse = await fetch(
      `${baseUrl}/pr/managed?owner=goddard-ai&repo=test-repo&prNumber=1`,
      { headers: { authorization: `Bearer ${bobSession.token}` } },
    )
    expect(foreignResponse.status).toBe(200)
    expect(await foreignResponse.json()).toEqual({ managed: false })
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

    expect(response.status).toBe(400)
    const payload = (await response.json()) as { message: string }
    expect(payload.message).toBe("Invalid request body")
  } finally {
    await server.close()
  }
})

test("websocket stream receives webhook events for a managed PR", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  let streamSocket: WebSocket | undefined

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    await postJson(
      `${baseUrl}/pr/create`,
      {
        owner: "goddard-ai",
        repo: "sdk",
        title: "Add CLI",
        head: "feat/cli",
        base: "main",
      },
      session.token,
    )

    streamSocket = await connectStreamSocket(baseUrl, session.token)
    const eventPromise = readFirstSocketMessage(streamSocket)

    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "looks good",
    })

    const parsed = (await eventPromise) as {
      id: number
      event: { type: string; reactionAdded: string }
    }
    expect(parsed.id).toBe(2)
    expect(parsed.event.type).toBe("comment")
    expect(parsed.event.reactionAdded).toBe("eyes")
  } finally {
    streamSocket?.close()
    await server.close()
  }
})

test("stream history returns managed events for the authenticated user in cursor order", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const alecFlow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: alecFlow.deviceCode,
      githubUsername: "alec",
    })
    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "bob" })
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      githubUsername: "bob",
    })

    await postJson(
      `${baseUrl}/pr/create`,
      { owner: "goddard-ai", repo: "sdk", title: "Alec PR", head: "feat/alec", base: "main" },
      alecSession.token,
    )
    await postJson(
      `${baseUrl}/pr/create`,
      { owner: "goddard-ai", repo: "daemon", title: "Bob PR", head: "feat/bob", base: "main" },
      bobSession.token,
    )

    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "looks good",
    })
    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "daemon",
      prNumber: 1,
      author: "teammate",
      body: "bob only",
    })

    const response = await fetch(`${baseUrl}/stream/history?after=1`, {
      headers: { authorization: `Bearer ${alecSession.token}` },
    })

    expect(response.status).toBe(200)
    const payload = (await response.json()) as {
      events: Array<{ id: number; event: { type: string; prNumber: number; repo: string } }>
    }
    expect(
      payload.events.map((event) => ({
        id: event.id,
        type: event.event.type,
        repo: event.event.repo,
        prNumber: event.event.prNumber,
      })),
    ).toEqual([{ id: 3, type: "comment", repo: "sdk", prNumber: 1 }])
  } finally {
    await server.close()
  }
})

test("unified stream only emits events for managed PRs owned by the authenticated user", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  let alecStream: WebSocket | undefined
  let bobStream: WebSocket | undefined

  try {
    const alecFlow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const alecSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: alecFlow.deviceCode,
      githubUsername: "alec",
    })
    const bobFlow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "bob" })
    const bobSession = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: bobFlow.deviceCode,
      githubUsername: "bob",
    })

    await postJson(
      `${baseUrl}/pr/create`,
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
      `${baseUrl}/pr/create`,
      {
        owner: "goddard-ai",
        repo: "daemon",
        title: "Bob PR",
        head: "feat/bob",
        base: "main",
      },
      bobSession.token,
    )

    alecStream = await connectStreamSocket(baseUrl, alecSession.token)
    bobStream = await connectStreamSocket(baseUrl, bobSession.token)
    const alecEventPromise = readFirstSocketMessage(alecStream)
    const bobEventPromise = assertNoSocketMessage(bobStream, 100)

    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "looks good",
    })

    const alecEvent = (await alecEventPromise) as { event: { prNumber: number } }
    expect(alecEvent.event.prNumber).toBe(1)
    await bobEventPromise
  } finally {
    alecStream?.close()
    bobStream?.close()
    await server.close()
  }
})

test("unified stream ignores webhook events for unmanaged PRs", async () => {
  const server = await startBackendServer(new InMemoryBackendControlPlane(), { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  let streamSocket: WebSocket | undefined

  try {
    const flow = await postJson(`${baseUrl}/auth/device/start`, { githubUsername: "alec" })
    const session = await postJson(`${baseUrl}/auth/device/complete`, {
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })

    streamSocket = await connectStreamSocket(baseUrl, session.token)

    await postJson(`${baseUrl}/webhooks/github`, {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 99,
      author: "teammate",
      body: "looks good",
    })

    await assertNoSocketMessage(streamSocket, 100)
  } finally {
    streamSocket?.close()
    await server.close()
  }
})

async function connectStreamSocket(baseUrl: string, token: string): Promise<WebSocket> {
  const socket = new WebSocket(createStreamUrl(baseUrl, token))

  await new Promise<void>((resolve, reject) => {
    socket.onopen = () => resolve()
    socket.onerror = () => reject(new Error("WebSocket connection failed"))
    socket.onclose = (event) => {
      reject(new Error(`WebSocket closed before opening (${event.code})`))
    }
  })

  return socket
}

async function readFirstSocketMessage(socket: WebSocket, timeoutMs = 1000): Promise<unknown> {
  return readWithTimeout(socket, timeoutMs)
}

async function assertNoSocketMessage(socket: WebSocket, timeoutMs: number): Promise<void> {
  try {
    await readFirstSocketMessage(socket, timeoutMs)
  } catch (error) {
    expect(String(error)).toMatch(/Timed out waiting for WebSocket message/)
    return
  }

  throw new Error(`Expected no WebSocket message within ${timeoutMs}ms`)
}

/** Waits for the next non-heartbeat WebSocket message and parses it as JSON. */
async function readWithTimeout(socket: WebSocket, timeoutMs: number): Promise<unknown> {
  return new Promise<unknown>((resolve, reject) => {
    const timeoutId = setTimeout(() => {
      cleanup()
      reject(new Error(`Timed out waiting for WebSocket message after ${timeoutMs}ms`))
    }, timeoutMs)

    const cleanup = () => {
      clearTimeout(timeoutId)
      socket.removeEventListener("message", onMessage)
      socket.removeEventListener("error", onError)
      socket.removeEventListener("close", onClose)
    }

    const onMessage = (event: MessageEvent) => {
      if (event.data === "pong") {
        return
      }

      cleanup()
      resolve(JSON.parse(String(event.data)))
    }

    const onError = () => {
      cleanup()
      reject(new Error("WebSocket error while waiting for message"))
    }

    const onClose = (event: CloseEvent) => {
      cleanup()
      reject(new Error(`WebSocket closed while waiting for message (${event.code})`))
    }

    socket.addEventListener("message", onMessage)
    socket.addEventListener("error", onError)
    socket.addEventListener("close", onClose)
  })
}

function createStreamUrl(baseUrl: string, token: string): string {
  const url = new URL("/stream", baseUrl)
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:"
  url.searchParams.set("token", token)
  return url.toString()
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
