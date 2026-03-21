import { InMemoryBackendControlPlane, startBackendServer } from "@goddard-ai/backend"
import { InMemoryTokenStorage } from "@goddard-ai/storage"
import { expect, test } from "vitest"
import { createBackendClient } from "../src/index.ts"

test("backend client creates PRs and checks managed status through rouzer route helpers", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  const tokenStorage = new InMemoryTokenStorage()

  try {
    const flow = controlPlane.startDeviceFlow({ githubUsername: "alec" })
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })
    await tokenStorage.setToken(session.token)

    const client = createBackendClient({ baseUrl, tokenStorage })
    const pr = await client.pr.create({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Add backend client",
      body: "Ship it",
      head: "feat/backend-client",
      base: "main",
    })

    expect(pr.number).toBe(1)
    await expect(
      client.pr.isManaged({ owner: "goddard-ai", repo: "sdk", prNumber: pr.number }),
    ).resolves.toBe(true)
  } finally {
    await server.close()
  }
})

test("backend client manages auth session state through token storage", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  const tokenStorage = new InMemoryTokenStorage()

  try {
    const client = createBackendClient({ baseUrl, tokenStorage })
    const start = await client.auth.startDeviceFlow({ githubUsername: "alec" })
    const session = await client.auth.completeDeviceFlow({
      deviceCode: start.deviceCode,
      githubUsername: "alec",
    })

    await expect(tokenStorage.getToken()).resolves.toBe(session.token)
    await expect(client.auth.whoami()).resolves.toEqual(session)

    await client.auth.logout()
    await expect(tokenStorage.getToken()).resolves.toBeNull()
  } finally {
    await server.close()
  }
})

test("backend client subscribes to the unified NDJSON stream", async () => {
  const tokenStorage = new InMemoryTokenStorage()
  await tokenStorage.setToken("tok_stream")
  const encoder = new TextEncoder()
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined

  const fetchImpl: typeof fetch = async (input) => {
    const url = String(input)
    if (url.endsWith("/stream")) {
      const stream = new ReadableStream<Uint8Array>({
        start(ctrl) {
          controller = ctrl
          ctrl.enqueue(encoder.encode("\n"))
        },
      })

      return new Response(stream, {
        status: 200,
        headers: {
          "content-type": "application/x-ndjson",
        },
      })
    }

    return new Response("not found", { status: 404 })
  }

  const client = createBackendClient({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage,
    fetchImpl,
  })
  const subscription = await client.stream.subscribe()
  const eventPromise = new Promise<unknown>((resolve) => {
    subscription.on("event", resolve)
  })

  controller?.enqueue(
    encoder.encode(
      `${JSON.stringify({
        id: 1,
        event: {
          type: "pr.created",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 1,
          title: "Stream me",
          author: "alec",
          createdAt: new Date().toISOString(),
        },
      })}\n`,
    ),
  )

  const event = (await eventPromise) as { type: string; prNumber: number }
  expect(event.type).toBe("pr.created")
  expect(event.prNumber).toBe(1)
  subscription.close()
})

test("backend client exposes event history and live stream message ids", async () => {
  const tokenStorage = new InMemoryTokenStorage()
  await tokenStorage.setToken("tok_history")
  const encoder = new TextEncoder()
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined

  const fetchImpl: typeof fetch = async (input) => {
    const url = String(input)
    if (url.endsWith("/stream")) {
      const stream = new ReadableStream<Uint8Array>({
        start(ctrl) {
          controller = ctrl
          ctrl.enqueue(encoder.encode("\n"))
        },
      })

      return new Response(stream, {
        status: 200,
        headers: {
          "content-type": "application/x-ndjson",
        },
      })
    }

    if (url.includes("/stream/history")) {
      return new Response(
        JSON.stringify({
          events: [
            {
              id: 2,
              createdAt: new Date().toISOString(),
              event: {
                type: "comment",
                owner: "goddard-ai",
                repo: "sdk",
                prNumber: 1,
                author: "teammate",
                body: "looks good",
                reactionAdded: "eyes",
                createdAt: new Date().toISOString(),
              },
            },
          ],
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      )
    }

    return new Response("not found", { status: 404 })
  }

  const client = createBackendClient({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage,
    fetchImpl,
  })
  const subscription = await client.stream.subscribe()
  const messagePromise = new Promise<unknown>((resolve) => {
    subscription.on("message", resolve)
  })

  controller?.enqueue(
    encoder.encode(
      `${JSON.stringify({
        id: 1,
        event: {
          type: "pr.created",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 1,
          title: "Stream me",
          author: "alec",
          createdAt: new Date().toISOString(),
        },
      })}\n`,
    ),
  )

  const message = (await messagePromise) as { id: number; event: { type: string } }
  expect(message.id).toBe(1)
  expect(message.event.type).toBe("pr.created")
  await expect(client.stream.history()).resolves.toEqual([
    expect.objectContaining({ id: 2, event: expect.objectContaining({ type: "comment" }) }),
  ])

  subscription.close()
})
