import { AsyncLocalStorage } from "node:async_hooks"
import { once } from "node:events"
import { request, type Server } from "node:http"
import { createServer as createTcpServer } from "node:net"
import { afterEach, describe, expect, test } from "bun:test"
import { getErrorMessage } from "radashi"
import { z } from "zod"

import {
  $type,
  http,
  IpcClientError,
  isIpcClientErrorForRegistry,
  ndjson,
  type HttpRouteTree,
  type IpcClientHookEvent,
} from "../src/index.ts"
import { createNodeClient } from "../src/node/client.ts"
import { createServer } from "../src/node/server.ts"

const routes = {
  ping: http.get("ping", {
    response: $type<{ ok: true }>(),
  }),
  echo: http.post("echo", {
    body: z.object({ text: z.string() }),
    response: $type<{ echoed: string }>(),
  }),
  add: http.post("add", {
    body: z.object({ a: z.number(), b: z.number() }),
    response: $type<{ sum: number }>(),
  }),
  systemAlert: http.get("system-alert", {
    response: ndjson.$type<{ message: string; level: "info" | "warn" | "error" }>(),
  }),
  userAlert: http.get("user-alert", {
    query: z.object({
      userId: z.string(),
    }),
    response: ndjson.$type<{
      userId: string
      message: string
    }>(),
  }),
} satisfies HttpRouteTree

const mathIpcErrors = {
  AddDisabled: {
    code: "math.add_disabled",
    details: z.strictObject({
      reason: z.string(),
    }),
  },
  AddUnauthorized: {
    code: "math.add_unauthorized",
    details: z.undefined(),
  },
} as const

const cleanups: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanups.length > 0) {
    await cleanups.pop()?.()
  }
})

async function createFixture() {
  const systemAlerts = createTestStream<{ message: string; level: "info" | "warn" | "error" }>()
  const userAlerts = createTestStream<{ userId: string; message: string }>()
  const ipcServer = createServer({
    port: 0,
    routes,
    handlers: {
      ping: () => ({ ok: true as const }),
      echo: ({ body: { text } }) => ({ echoed: text }),
      add: ({ body: { a, b } }) => ({ sum: a + b }),
      systemAlert: ({ request }) => systemAlerts.subscribe(() => true, request.signal),
      userAlert: ({ query, request }) => {
        if (query.userId === "blocked-user") {
          throw new IpcClientError("User alerts are disabled for blocked-user")
        }
        return userAlerts.subscribe((payload) => payload.userId === query.userId, request.signal)
      },
    },
  })

  await once(ipcServer.server, "listening")
  const address = readTcpAddress(ipcServer.server)

  cleanups.push(async () => {
    await new Promise<void>((resolve, reject) => {
      ipcServer.server.close((error) => {
        if (error) {
          reject(error)
          return
        }
        resolve()
      })
    })
  })

  return {
    address,
    client: createNodeClient(address, routes),
    publishSystemAlert: systemAlerts.publish,
    publishUserAlert: userAlerts.publish,
    waitForSystemAlertSubscription: systemAlerts.waitForSubscriber,
    waitForUserAlertSubscription: userAlerts.waitForSubscriber,
  }
}

function createTestStream<TPayload>() {
  const listeners = new Set<(payload: TPayload) => void>()
  const subscriberWaiters = new Set<() => void>()

  return {
    publish(payload: TPayload) {
      for (const listener of listeners) {
        listener(payload)
      }
    },
    waitForSubscriber() {
      if (listeners.size > 0) {
        return Promise.resolve()
      }

      return new Promise<void>((resolve) => {
        subscriberWaiters.add(resolve)
      })
    },
    async *subscribe(filter: (payload: TPayload) => boolean, signal: AbortSignal) {
      const queue: TPayload[] = []
      let wake: (() => void) | undefined
      const listener = (payload: TPayload) => {
        if (!filter(payload)) {
          return
        }
        queue.push(payload)
        wake?.()
      }
      const abort = () => {
        wake?.()
      }

      listeners.add(listener)
      for (const resolve of subscriberWaiters) {
        resolve()
      }
      subscriberWaiters.clear()
      signal.addEventListener("abort", abort)
      try {
        while (!signal.aborted) {
          const payload = queue.shift()
          if (payload) {
            yield payload
            continue
          }
          await new Promise<void>((resolve) => {
            wake = resolve
          })
          wake = undefined
        }
      } finally {
        signal.removeEventListener("abort", abort)
        listeners.delete(listener)
      }
    },
  }
}

async function requestRaw(
  address: { hostname: string; port: number },
  input: { method: string; path: string; body?: unknown; headers?: Record<string, string> },
) {
  const payload = input.body === undefined ? undefined : JSON.stringify(input.body)

  return new Promise<{
    statusCode: number
    headers: Record<string, string | string[]>
    body: string
  }>((resolve, reject) => {
    const req = request(
      {
        hostname: address.hostname,
        port: address.port,
        path: input.path,
        method: input.method,
        headers:
          payload === undefined
            ? input.headers
            : {
                ...input.headers,
                "Content-Type": "application/json",
                "Content-Length": Buffer.byteLength(payload),
              },
      },
      (res) => {
        let responseBody = ""
        res.setEncoding("utf8")
        res.on("data", (chunk) => {
          responseBody += chunk
        })
        res.on("end", () => {
          resolve({
            statusCode: res.statusCode ?? 0,
            headers: res.headers as Record<string, string | string[]>,
            body: responseBody,
          })
        })
      },
    )

    req.on("error", reject)
    if (payload !== undefined) {
      req.write(payload)
    }
    req.end()
  })
}

function readTcpAddress(server: Server) {
  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("IPC server did not bind to a TCP port")
  }

  return {
    hostname: address.address,
    port: address.port,
  }
}

async function createBrowserAccessFixture(
  allowedOrigins = ["https://app.goddardai.org"],
  isAllowedOrigin?: (origin: string) => boolean,
) {
  let pingCount = 0
  const ipcServer = createServer({
    port: 0,
    routes,
    browserAccess: {
      allowedOrigins,
      isAllowedOrigin,
    },
    handlers: {
      ping: () => {
        pingCount += 1
        return { ok: true as const }
      },
      echo: ({ body: { text } }) => ({ echoed: text }),
      add: ({ body: { a, b } }) => ({ sum: a + b }),
      systemAlert: () => [],
      userAlert: () => [],
    },
  })

  await once(ipcServer.server, "listening")
  const address = readTcpAddress(ipcServer.server)

  cleanups.push(async () => {
    await new Promise<void>((resolve, reject) => {
      ipcServer.server.close((error) => {
        if (error) {
          reject(error)
          return
        }
        resolve()
      })
    })
  })

  return {
    address,
    readPingCount: () => pingCount,
  }
}

async function getUnusedTcpAddress() {
  const server = createTcpServer()
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject)
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject)
      resolve()
    })
  })

  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("TCP probe did not bind to a port")
  }

  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error)
        return
      }

      resolve()
    })
  })

  return {
    hostname: address.address,
    port: address.port,
  }
}

async function readFirst<T>(stream: AsyncIterable<T>) {
  for await (const item of stream) {
    return item
  }
  throw new Error("Stream ended before yielding an item")
}

describe("core/ipc", () => {
  test("sends validated request/response messages over TCP", async () => {
    const { client } = await createFixture()

    await expect(client.ping()).resolves.toEqual({ ok: true })
    await expect(client.echo({ text: "hello" })).resolves.toEqual({ echoed: "hello" })
    await expect(client.add({ a: 2, b: 3 })).resolves.toEqual({ sum: 5 })
  })

  test("createNodeClient accepts request lifecycle hooks", async () => {
    const { address } = await createFixture()
    const events: IpcClientHookEvent[] = []
    const client = createNodeClient(address, routes, {
      ipcHook: (event) => {
        events.push(event)
      },
    })

    await expect(client.ping()).resolves.toEqual({ ok: true })

    expect(events).toHaveLength(2)
    expect(events[0]).toMatchObject({
      type: "request.start",
      routeName: "ping",
      method: "GET",
      pathPattern: "/ping",
    })
    expect(events[1]).toMatchObject({
      type: "request.success",
      routeName: "ping",
      response: { ok: true },
      status: 200,
    })
  })

  test("request lifecycle hooks observe client-side validation failures", async () => {
    const { address } = await createFixture()
    const events: IpcClientHookEvent[] = []
    const client = createNodeClient(address, routes, {
      ipcHook: (event) => {
        events.push(event)
      },
    })

    await expect(client.add({ a: 2, b: "3" } as never)).rejects.toThrow()

    expect(events[0]).toMatchObject({
      type: "request.start",
      routeName: "add",
      payload: { a: 2, b: "3" },
    })
    expect(events[1]).toMatchObject({
      type: "request.error",
      routeName: "add",
    })
    expect(events[0]?.opId).toBe(events[1]?.opId)
    expect(events[1]).not.toHaveProperty("status")
    expect(events[1]?.type === "request.error" ? getErrorMessage(events[1].error) : "").toContain(
      "Invalid",
    )
  })

  test("rejects invalid request payloads before they cross the process boundary", async () => {
    const { client } = await createFixture()

    await expect(client.add({ a: 2, b: "3" } as never)).rejects.toThrow()
  })

  test("describes invalid raw request bodies", async () => {
    const { address } = await createFixture()

    const response = await requestRaw(address, { method: "POST", path: "/echo" })
    expect(response.statusCode).toBe(400)
    expect(JSON.parse(response.body)).toMatchObject({ message: "Invalid request body" })
  })

  test("returns not found for unknown routes", async () => {
    const { address } = await createFixture()

    await expect(
      requestRaw(address, { method: "POST", path: "/missing", body: {} }),
    ).resolves.toMatchObject({
      statusCode: 404,
      body: "Not Found",
    })
  })

  test("preserves non-browser requests when browser access is configured", async () => {
    const { address, readPingCount } = await createBrowserAccessFixture()

    const response = await requestRaw(address, { method: "GET", path: "/ping" })

    expect(response.statusCode).toBe(200)
    expect(JSON.parse(response.body)).toEqual({ ok: true })
    expect(response.headers["access-control-allow-origin"]).toBeUndefined()
    expect(readPingCount()).toBe(1)
  })

  test("adds CORS headers for configured browser origins", async () => {
    const { address, readPingCount } = await createBrowserAccessFixture()

    const response = await requestRaw(address, {
      method: "GET",
      path: "/ping",
      headers: {
        Origin: "https://app.goddardai.org",
      },
    })

    expect(response.statusCode).toBe(200)
    expect(JSON.parse(response.body)).toEqual({ ok: true })
    expect(response.headers["access-control-allow-origin"]).toBe("https://app.goddardai.org")
    expect(response.headers["access-control-allow-credentials"]).toBeUndefined()
    expect(readPingCount()).toBe(1)
  })

  test("adds CORS headers for predicate-allowed browser origins", async () => {
    const { address, readPingCount } = await createBrowserAccessFixture(
      [],
      (origin) => origin === "http://127.0.0.1:5173",
    )

    const response = await requestRaw(address, {
      method: "GET",
      path: "/ping",
      headers: {
        Origin: "http://127.0.0.1:5173",
      },
    })

    expect(response.statusCode).toBe(200)
    expect(JSON.parse(response.body)).toEqual({ ok: true })
    expect(response.headers["access-control-allow-origin"]).toBe("http://127.0.0.1:5173")
    expect(readPingCount()).toBe(1)
  })

  test("answers private-network preflights for configured browser origins", async () => {
    const { address, readPingCount } = await createBrowserAccessFixture()

    const response = await requestRaw(address, {
      method: "OPTIONS",
      path: "/ping",
      headers: {
        Origin: "https://app.goddardai.org",
        "Access-Control-Request-Method": "GET",
        "Access-Control-Request-Private-Network": "true",
      },
    })

    expect(response.statusCode).toBe(204)
    expect(response.headers["access-control-allow-origin"]).toBe("https://app.goddardai.org")
    expect(response.headers["access-control-allow-private-network"]).toBe("true")
    expect(response.headers["access-control-allow-methods"]).toBe("GET, POST, OPTIONS")
    expect(response.headers["access-control-allow-headers"]).toBe("authorization, content-type")
    expect(response.headers["access-control-allow-credentials"]).toBeUndefined()
    expect(response.body).toBe("")
    expect(readPingCount()).toBe(0)
  })

  test("fails closed for unconfigured browser origins before handlers run", async () => {
    const { address, readPingCount } = await createBrowserAccessFixture()

    const response = await requestRaw(address, {
      method: "GET",
      path: "/ping",
      headers: {
        Origin: "https://evil.example",
      },
    })

    expect(response.statusCode).toBe(403)
    expect(response.headers["access-control-allow-origin"]).toBeUndefined()
    expect(JSON.parse(response.body)).toEqual({ error: "Forbidden" })
    expect(readPingCount()).toBe(0)
  })

  test("fails closed for malformed, null, missing, and unconfigured preflight origins", async () => {
    const { address, readPingCount } = await createBrowserAccessFixture()
    const originCases: Array<Record<string, string>> = [
      {},
      { Origin: "null" },
      { Origin: "https://[invalid" },
      { Origin: "https://evil.example" },
    ]

    for (const headers of originCases) {
      const response = await requestRaw(address, {
        method: "OPTIONS",
        path: "/ping",
        headers: {
          ...headers,
          "Access-Control-Request-Method": "GET",
          "Access-Control-Request-Private-Network": "true",
        },
      })

      expect(response.statusCode).toBe(403)
      expect(response.headers["access-control-allow-origin"]).toBeUndefined()
      expect(response.headers["access-control-allow-private-network"]).toBeUndefined()
      expect(JSON.parse(response.body)).toEqual({ error: "Forbidden" })
    }

    expect(readPingCount()).toBe(0)
  })

  test("rejects wildcard browser access configuration", () => {
    expect(() =>
      createServer({
        port: 0,
        routes,
        browserAccess: {
          allowedOrigins: ["*"],
        },
        handlers: {
          ping: () => ({ ok: true as const }),
          echo: ({ body: { text } }) => ({ echoed: text }),
          add: ({ body: { a, b } }) => ({ sum: a + b }),
          systemAlert: () => [],
          userAlert: () => [],
        },
      }),
    ).toThrow("Browser access origin must be explicit: *")
  })

  test("rejects invalid browser access hosts before handlers run", async () => {
    const { address, readPingCount } = await createBrowserAccessFixture()

    const response = await requestRaw(address, {
      method: "GET",
      path: "/ping",
      headers: {
        Host: `evil.example:${address.port}`,
        Origin: "https://app.goddardai.org",
      },
    })

    expect(response.statusCode).toBe(403)
    expect(response.headers["access-control-allow-origin"]).toBeUndefined()
    expect(JSON.parse(response.body)).toEqual({ error: "Forbidden" })
    expect(readPingCount()).toBe(0)
  })

  test("creates request context and fires request lifecycle hooks", async () => {
    const requestContext = new AsyncLocalStorage<{ traceId: string }>()
    const received: Array<{ name: string; payload: unknown; traceId: string }> = []
    const responded: Array<{
      name: string
      payload: unknown
      response: unknown
      durationMs: number
      traceId: string
    }> = []
    const handlerContexts: string[] = []
    let requestCount = 0
    const readTraceId = () => {
      const traceId = requestContext.getStore()?.traceId
      if (!traceId) {
        throw new Error("Missing request trace ID")
      }

      return traceId
    }
    const ipcServer = createServer({
      port: 0,
      routes,
      handlers: {
        ping: () => {
          handlerContexts.push(readTraceId())
          return { ok: true as const }
        },
        echo: ({ body: { text } }) => {
          const traceId = readTraceId()
          handlerContexts.push(traceId)
          return { echoed: `${text}:${traceId}` }
        },
        add: ({ body: { a, b } }) => {
          handlerContexts.push(readTraceId())
          return { sum: a + b }
        },
        systemAlert: () => [],
        userAlert: () => [],
      },
      runHandler: ({ name }, handler) =>
        requestContext.run(
          {
            traceId: `${name}-${String(++requestCount)}`,
          },
          handler,
        ),
      onRequestReceived: ({ name, payload }) => {
        received.push({ name, payload, traceId: readTraceId() })
      },
      onResponseSent: ({ name, payload, response, durationMs }) => {
        responded.push({ name, payload, response, durationMs, traceId: readTraceId() })
      },
    })

    await once(ipcServer.server, "listening")
    const address = readTcpAddress(ipcServer.server)
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    })

    const client = createNodeClient(address, routes)
    await expect(client.ping()).resolves.toEqual({ ok: true })
    await expect(client.echo({ text: "hello" })).resolves.toEqual({
      echoed: "hello:echo-2",
    })

    expect(received).toEqual([
      { name: "ping", payload: undefined, traceId: "ping-1" },
      { name: "echo", payload: { text: "hello" }, traceId: "echo-2" },
    ])
    expect(responded[0]).toMatchObject({
      name: "ping",
      payload: undefined,
      response: { ok: true },
      traceId: "ping-1",
    })
    expect(responded[1]).toMatchObject({
      name: "echo",
      payload: { text: "hello" },
      response: { echoed: "hello:echo-2" },
      traceId: "echo-2",
    })
    expect(responded.every((entry) => entry.durationMs >= 0)).toBe(true)
    expect(handlerContexts).toEqual(["ping-1", "echo-2"])
  })

  test("streams ndjson events to subscribed node clients", async () => {
    const { client, publishSystemAlert, waitForSystemAlertSubscription } = await createFixture()
    const abortController = new AbortController()
    cleanups.push(async () => {
      abortController.abort()
    })

    const stream = await client.systemAlert(undefined, { signal: abortController.signal })
    const alertPromise = readFirst(stream)

    await waitForSystemAlertSubscription()
    publishSystemAlert({ message: "Heads up", level: "warn" })

    await expect(alertPromise).resolves.toEqual({ message: "Heads up", level: "warn" })
  })

  test("applies stream filters on the server side", async () => {
    const { client, publishUserAlert, waitForUserAlertSubscription } = await createFixture()
    const abortController = new AbortController()
    const delivered: Array<{
      userId: string
      message: string
    }> = []
    let resolveDelivered: (() => void) | undefined
    const deliveredPromise = new Promise<void>((resolve) => {
      resolveDelivered = resolve
    })
    cleanups.push(async () => {
      abortController.abort()
    })

    const stream = await client.userAlert({ userId: "user-1" }, { signal: abortController.signal })
    const readPromise = (async () => {
      for await (const payload of stream) {
        delivered.push(payload)
        resolveDelivered?.()
      }
    })()
    cleanups.push(async () => {
      abortController.abort()
      await readPromise.catch(() => {})
    })

    await waitForUserAlertSubscription()
    publishUserAlert({ userId: "user-2", message: "skip me" })
    publishUserAlert({ userId: "user-1", message: "deliver me" })
    await deliveredPromise
    await new Promise((resolve) => setTimeout(resolve, 25))

    expect(delivered).toEqual([{ userId: "user-1", message: "deliver me" }])
  })

  test("rejects stream filters when the server-side validator fails", async () => {
    const { client } = await createFixture()

    await expect(client.userAlert({ userId: "blocked-user" })).rejects.toThrow(
      "User alerts are disabled for blocked-user",
    )
  })

  test("stream handlers subscribe and unsubscribe exactly once", async () => {
    const events: Array<{
      phase: "subscribe" | "unsubscribe"
      filter: unknown
    }> = []
    const ipcServer = createServer({
      port: 0,
      routes,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ body: { text } }) => ({ echoed: text }),
        add: ({ body: { a, b } }) => ({ sum: a + b }),
        systemAlert: () => [],
        userAlert: ({ query, request }) => {
          events.push({ phase: "subscribe", filter: query })
          return (async function* () {
            const signal = request.signal
            let wake: (() => void) | undefined
            const abort = () => {
              wake?.()
            }
            signal.addEventListener("abort", abort)
            try {
              yield { userId: query.userId, message: "ready" }
              while (!signal.aborted) {
                await new Promise<void>((resolve) => {
                  wake = resolve
                })
                wake = undefined
              }
            } finally {
              signal.removeEventListener("abort", abort)
              events.push({ phase: "unsubscribe", filter: query })
            }
          })()
        },
      },
    })

    await once(ipcServer.server, "listening")
    const address = readTcpAddress(ipcServer.server)
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    })

    const client = createNodeClient(address, routes)
    const abortController = new AbortController()
    const stream = await client.userAlert({ userId: "user-1" }, { signal: abortController.signal })

    await expect(readFirst(stream)).resolves.toEqual({ userId: "user-1", message: "ready" })
    abortController.abort()
    abortController.abort()
    await new Promise((resolve) => setTimeout(resolve, 25))

    expect(events).toEqual([
      {
        phase: "subscribe",
        filter: { userId: "user-1" },
      },
      {
        phase: "unsubscribe",
        filter: { userId: "user-1" },
      },
    ])
  })

  test("fires request-failed hooks when a handler throws", async () => {
    const requestContext = new AsyncLocalStorage<{ traceId: string }>()
    const failures: Array<{
      name: string
      payload: unknown
      errorMessage: string
      durationMs: number
      traceId: string
    }> = []
    const ipcServer = createServer({
      port: 0,
      routes,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ body: { text } }) => ({ echoed: text }),
        add: () => {
          throw new Error("handler exploded")
        },
        systemAlert: () => [],
        userAlert: () => [],
      },
      runHandler: (_input, handler) => requestContext.run({ traceId: "trace-add" }, handler),
      onRequestFailed: ({ name, payload, error, durationMs }) => {
        const traceId = requestContext.getStore()?.traceId
        if (!traceId) {
          throw new Error("Missing request trace ID")
        }

        failures.push({
          name,
          payload,
          errorMessage: getErrorMessage(error),
          durationMs,
          traceId,
        })
      },
    })

    await once(ipcServer.server, "listening")
    const address = readTcpAddress(ipcServer.server)
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    })

    const client = createNodeClient(address, routes)
    await expect(client.add({ a: 1, b: 2 })).rejects.toThrow("Internal server error")

    expect(failures).toHaveLength(1)
    expect(failures[0]).toMatchObject({
      name: "add",
      payload: { a: 1, b: 2 },
      errorMessage: "handler exploded",
      traceId: "trace-add",
    })
    expect(failures[0]?.durationMs).toBeGreaterThanOrEqual(0)
  })

  test("returns client-visible handler failures unchanged", async () => {
    const ipcServer = createServer({
      port: 0,
      routes,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ body: { text } }) => ({ echoed: text }),
        add: () => {
          throw new IpcClientError("Add is disabled")
        },
        systemAlert: () => [],
        userAlert: () => [],
      },
    })

    await once(ipcServer.server, "listening")
    const address = readTcpAddress(ipcServer.server)
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    })

    const client = createNodeClient(address, routes)
    await expect(client.add({ a: 1, b: 2 })).rejects.toThrow("Add is disabled")
  })

  test("preserves structured client-visible handler failures", async () => {
    const ipcServer = createServer({
      port: 0,
      routes,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ body: { text } }) => ({ echoed: text }),
        add: () => {
          throw new IpcClientError({
            code: "math.add_disabled",
            details: { reason: "maintenance" },
          })
        },
        systemAlert: () => [],
        userAlert: () => [],
      },
    })

    await once(ipcServer.server, "listening")
    const address = readTcpAddress(ipcServer.server)
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    })

    const client = createNodeClient(address, routes)
    try {
      await client.add({ a: 1, b: 2 })
      throw new Error("Expected client.add to reject")
    } catch (error) {
      expect(error).toBeInstanceOf(IpcClientError)
      expect(error).toMatchObject({
        code: "math.add_disabled",
        details: { reason: "maintenance" },
        message: "IPC request failed: math.add_disabled",
      })
    }

    const response = await requestRaw(address, {
      method: "POST",
      path: "/add",
      body: { a: 1, b: 2 },
    })

    expect(response.statusCode).toBe(400)
    expect(JSON.parse(response.body)).toEqual({
      error: {
        code: "math.add_disabled",
        details: { reason: "maintenance" },
      },
    })
  })

  test("narrows IPC client errors by registry", () => {
    const error: unknown = new IpcClientError({
      code: "math.add_disabled",
      details: { reason: "maintenance" },
    })

    if (!isIpcClientErrorForRegistry(error, mathIpcErrors)) {
      throw new Error("Expected math IPC error")
    }

    switch (error.code) {
      case "math.add_disabled":
        expect(error.details).toEqual({ reason: "maintenance" })
        break
      case "math.add_unauthorized":
        expect(error.details).toBeUndefined()
        break
      default:
        expect.unreachable()
    }
  })

  test("rejects IPC client errors outside a registry", () => {
    expect(
      isIpcClientErrorForRegistry(
        new IpcClientError({
          code: "math.subtract_disabled",
          details: { reason: "maintenance" },
        }),
        mathIpcErrors,
      ),
    ).toBe(false)
  })

  test("rejects IPC client errors with invalid registry details", () => {
    expect(
      isIpcClientErrorForRegistry(
        new IpcClientError({
          code: "math.add_disabled",
          details: { retryAfter: 30 },
        }),
        mathIpcErrors,
      ),
    ).toBe(false)
  })

  test("rewords missing IPC request failures", async () => {
    const missingAddress = await getUnusedTcpAddress()
    const client = createNodeClient(missingAddress, routes)

    await expect(client.ping()).rejects.toThrow(
      `Could not connect to IPC server at http://${missingAddress.hostname}:${missingAddress.port}/.`,
    )
  })

  test("rewords missing IPC stream failures", async () => {
    const missingAddress = await getUnusedTcpAddress()
    const client = createNodeClient(missingAddress, routes)

    await expect(client.systemAlert()).rejects.toThrow(
      `Could not connect to IPC server at http://${missingAddress.hostname}:${missingAddress.port}/.`,
    )
  })

  test("returns generic raw errors for unexpected handler failures", async () => {
    const ipcServer = createServer({
      port: 0,
      routes,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ body: { text } }) => ({ echoed: text }),
        add: () => {
          throw new Error("handler exploded")
        },
        systemAlert: () => [],
        userAlert: () => [],
      },
    })

    await once(ipcServer.server, "listening")
    const address = readTcpAddress(ipcServer.server)
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    })

    const response = await requestRaw(address, {
      method: "POST",
      path: "/add",
      body: { a: 1, b: 2 },
    })

    expect(response).toMatchObject({
      statusCode: 500,
      body: JSON.stringify({ error: "Internal server error" }),
    })
  })
})
