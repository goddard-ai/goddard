import { expect, test } from "bun:test"

import {
  BrowserDaemonAuthorizationError,
  BrowserDaemonConnectionError,
  createBrowserDaemonIpcClient,
} from "../src/browser/index.ts"

test("createBrowserDaemonIpcClient attaches bearer tokens to browser fetch requests", async () => {
  const requests: Request[] = []
  const client = createBrowserDaemonIpcClient({
    daemonUrl: "http://127.0.0.1:49827/",
    token: () => "browser-token",
    fetch: (async (input, init) => {
      requests.push(new Request(input, init))
      return Response.json({ ok: true })
    }) as typeof fetch,
  })

  await expect(client.daemon.health()).resolves.toEqual({ ok: true })

  expect(requests).toHaveLength(1)
  expect(requests[0].url).toBe("http://127.0.0.1:49827/daemon/health")
  expect(requests[0].headers.get("authorization")).toBe("Bearer browser-token")
})

test("createBrowserDaemonIpcClient resolves lazy browser access before requests", async () => {
  const requests: Request[] = []
  const client = createBrowserDaemonIpcClient({
    access: () => ({
      daemonUrl: "http://127.0.0.1:49827/",
      token: "access-token",
    }),
    fetch: (async (input, init) => {
      requests.push(new Request(input, init))
      return Response.json({ ok: true })
    }) as typeof fetch,
  })

  await expect(client.daemon.health()).resolves.toEqual({ ok: true })

  expect(requests).toHaveLength(1)
  expect(requests[0].url).toBe("http://127.0.0.1:49827/daemon/health")
  expect(requests[0].headers.get("authorization")).toBe("Bearer access-token")
})

test("createBrowserDaemonIpcClient refreshes lazy browser access once after authorization denial", async () => {
  const tokens = ["expired-token", "fresh-token"]
  const requests: Request[] = []
  let accessCalls = 0
  const client = createBrowserDaemonIpcClient({
    access: () => {
      accessCalls += 1
      return {
        daemonUrl: "http://127.0.0.1:49827/",
        token: tokens.shift(),
      }
    },
    fetch: (async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      if (request.headers.get("authorization") === "Bearer expired-token") {
        return Response.json({ error: "Forbidden" }, { status: 403 })
      }

      return Response.json({ ok: true })
    }) as typeof fetch,
  })

  await expect(client.daemon.health()).resolves.toEqual({ ok: true })

  expect(accessCalls).toBe(2)
  expect(requests.map((request) => request.headers.get("authorization"))).toEqual([
    "Bearer expired-token",
    "Bearer fresh-token",
  ])
})

test("createBrowserDaemonIpcClient exposes denied browser authorization as a stable error", async () => {
  const client = createBrowserDaemonIpcClient({
    daemonUrl: "http://127.0.0.1:49827/",
    token: undefined,
    fetch: (async () =>
      Response.json({ error: "Forbidden" }, { status: 403 })) as unknown as typeof fetch,
  })

  await expect(client.daemon.health()).rejects.toBeInstanceOf(BrowserDaemonAuthorizationError)
  await expect(client.daemon.health()).rejects.toMatchObject({
    status: 403,
    message: "Forbidden",
  })
})

test("createBrowserDaemonIpcClient exposes loopback connection failures distinctly", async () => {
  const client = createBrowserDaemonIpcClient({
    daemonUrl: "http://127.0.0.1:49827/",
    fetch: (async () => {
      throw new TypeError("fetch failed")
    }) as unknown as typeof fetch,
  })

  await expect(client.daemon.health()).rejects.toBeInstanceOf(BrowserDaemonConnectionError)
})

test("createBrowserDaemonIpcClient consumes daemon ndjson streams in browser runtimes", async () => {
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(`${JSON.stringify({ type: "started" })}\n`))
      controller.enqueue(new TextEncoder().encode(`${JSON.stringify({ type: "finished" })}\n`))
      controller.close()
    },
  })
  const client = createBrowserDaemonIpcClient({
    daemonUrl: "http://127.0.0.1:49827/",
    token: () => "browser-token",
    fetch: (async () =>
      new Response(stream, {
        headers: {
          "content-type": "application/x-ndjson",
        },
      })) as unknown as typeof fetch,
  })

  const events: unknown[] = []
  for await (const event of await client.events.stream(
    { names: ["session.lifecycle.updated", "session.lifecycle.deleted"] },
    {
      signal: new AbortController().signal,
    },
  )) {
    events.push(event)
  }

  expect(events).toEqual([{ type: "started" }, { type: "finished" }])
})
