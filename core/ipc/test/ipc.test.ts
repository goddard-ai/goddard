import { AsyncLocalStorage } from "node:async_hooks"
import { once } from "node:events"
import { request, type Server } from "node:http"
import { createServer as createTcpServer } from "node:net"
import { afterEach, describe, expect, test, vi } from "bun:test"
import { z } from "zod"

import { $type, IpcClientError, type IpcSchema } from "../src/index.ts"
import { createNodeClient } from "../src/node/client.ts"
import { createServer } from "../src/node/server.ts"

const schema = {
  requests: {
    ping: {
      response: $type<{ ok: true }>(),
    },
    echo: {
      payload: z.object({ text: z.string() }),
      response: $type<{ echoed: string }>(),
    },
    add: {
      payload: z.object({ a: z.number(), b: z.number() }),
      response: $type<{ sum: number }>(),
    },
  },
  streams: {
    systemAlert: $type<{ message: string; level: "info" | "warn" | "error" }>(),
    userAlert: {
      payload: $type<{
        userId: string
        message: string
      }>(),
      filter: z.object({
        userId: z.string(),
      }),
    },
  },
} satisfies IpcSchema

const cleanups: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanups.length > 0) {
    await cleanups.pop()?.()
  }
})

async function createFixture() {
  const ipcServer = createServer({
    port: 0,
    schema,
    handlers: {
      ping: () => ({ ok: true as const }),
      echo: ({ text }) => ({ echoed: text }),
      add: ({ a, b }) => ({ sum: a + b }),
    },
    beforeSubscribe: ({ name, filter }) => {
      if (name === "userAlert" && filter?.userId === "blocked-user") {
        throw new IpcClientError("User alerts are disabled for blocked-user")
      }
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
    client: createNodeClient(address, schema),
    publish: ipcServer.publish,
  }
}

async function postRaw(address: { hostname: string; port: number }, body: unknown) {
  const payload = JSON.stringify(body)

  return new Promise<{ statusCode: number; body: string }>((resolve, reject) => {
    const req = request(
      {
        hostname: address.hostname,
        port: address.port,
        path: "/",
        method: "POST",
        headers: {
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
            body: responseBody,
          })
        })
      },
    )

    req.on("error", reject)
    req.write(payload)
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

describe("core/ipc", () => {
  test("sends validated request/response messages over TCP", async () => {
    const { client } = await createFixture()

    await expect(client.send("ping")).resolves.toEqual({ ok: true })
    await expect(client.send("echo", { text: "hello" })).resolves.toEqual({ echoed: "hello" })
    await expect(client.send("add", { a: 2, b: 3 })).resolves.toEqual({ sum: 5 })
  })

  test("rejects invalid request payloads before they cross the process boundary", async () => {
    const { client } = await createFixture()

    await expect(client.send("add", { a: 2, b: "3" } as never)).rejects.toThrow()
  })

  test("describes the expected payload shape when a request payload object is omitted", async () => {
    const { client } = await createFixture()
    const expectedMessage = ["Expected input shape:", "{", "  text: string", "}"].join("\n")

    await expect((client.send as any)("echo")).rejects.toThrow(expectedMessage)
  })

  test("returns a structured error for unknown requests", async () => {
    const { address } = await createFixture()

    await expect(postRaw(address, { name: "missing", payload: {} })).resolves.toEqual({
      statusCode: 400,
      body: JSON.stringify({ error: "Unknown request: missing" }),
    })
  })

  test("returns the expected payload shape when a raw request omits its payload object", async () => {
    const { address } = await createFixture()
    const expectedMessage = ["Expected input shape:", "{", "  text: string", "}"].join("\n")

    await expect(postRaw(address, { name: "echo" })).resolves.toEqual({
      statusCode: 400,
      body: JSON.stringify({ error: expectedMessage }),
    })
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
      schema,
      handlers: {
        ping: () => {
          handlerContexts.push(readTraceId())
          return { ok: true as const }
        },
        echo: ({ text }) => {
          const traceId = readTraceId()
          handlerContexts.push(traceId)
          return { echoed: `${text}:${traceId}` }
        },
        add: ({ a, b }) => {
          handlerContexts.push(readTraceId())
          return { sum: a + b }
        },
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

    const client = createNodeClient(address, schema)
    await expect(client.send("ping")).resolves.toEqual({ ok: true })
    await expect(client.send("echo", { text: "hello" })).resolves.toEqual({
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
    const { client, publish } = await createFixture()

    let resolveAlert:
      | ((payload: { message: string; level: "info" | "warn" | "error" }) => void)
      | null = null
    const alertPromise = new Promise<{ message: string; level: "info" | "warn" | "error" }>(
      (resolve) => {
        resolveAlert = resolve
      },
    )
    const unsubscribe = await client.subscribe("systemAlert", (payload) => {
      resolveAlert?.(payload)
    })
    cleanups.push(async () => {
      unsubscribe()
    })

    await new Promise((resolve) => setTimeout(resolve, 25))
    publish("systemAlert", { message: "Heads up", level: "warn" })

    await expect(alertPromise).resolves.toEqual({ message: "Heads up", level: "warn" })
  })

  test("applies stream filters on the server side", async () => {
    const { client, publish } = await createFixture()
    const onMessage = vi.fn()

    const unsubscribe = await client.subscribe(
      { name: "userAlert", filter: { userId: "user-1" } },
      onMessage,
    )
    cleanups.push(async () => {
      unsubscribe()
    })

    await new Promise((resolve) => setTimeout(resolve, 25))
    publish("userAlert", { userId: "user-2", message: "skip me" })
    publish("userAlert", { userId: "user-1", message: "deliver me" })
    await new Promise((resolve) => setTimeout(resolve, 25))

    expect(onMessage).toHaveBeenCalledTimes(1)
    expect(onMessage).toHaveBeenCalledWith({ userId: "user-1", message: "deliver me" })
  })

  test("rejects stream filters when the server-side validator fails", async () => {
    const { client } = await createFixture()

    await expect(
      client.subscribe({ name: "userAlert", filter: { userId: "blocked-user" } }, () => {}),
    ).rejects.toThrow("User alerts are disabled for blocked-user")
  })

  test("fires stream lifecycle hooks and unsubscribes exactly once", async () => {
    const events: Array<{
      phase: "subscribe" | "unsubscribe"
      name: string
      filter: unknown
    }> = []
    const ipcServer = createServer({
      port: 0,
      schema,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ text }) => ({ echoed: text }),
        add: ({ a, b }) => ({ sum: a + b }),
      },
      afterSubscribe: ({ name, filter }) => {
        events.push({ phase: "subscribe", name, filter })
      },
      afterUnsubscribe: ({ name, filter }) => {
        events.push({ phase: "unsubscribe", name, filter })
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

    const client = createNodeClient(address, schema)
    const unsubscribe = await client.subscribe(
      { name: "userAlert", filter: { userId: "user-1" } },
      () => {},
    )

    await new Promise((resolve) => setTimeout(resolve, 25))
    unsubscribe()
    unsubscribe()
    await new Promise((resolve) => setTimeout(resolve, 25))

    expect(events).toEqual([
      {
        phase: "subscribe",
        name: "userAlert",
        filter: { userId: "user-1" },
      },
      {
        phase: "unsubscribe",
        name: "userAlert",
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
      schema,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ text }) => ({ echoed: text }),
        add: () => {
          throw new Error("handler exploded")
        },
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
          errorMessage: error instanceof Error ? error.message : String(error),
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

    const client = createNodeClient(address, schema)
    await expect(client.send("add", { a: 1, b: 2 })).rejects.toThrow("Internal server error")

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
      schema,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ text }) => ({ echoed: text }),
        add: () => {
          throw new IpcClientError("Add is disabled")
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

    const client = createNodeClient(address, schema)
    await expect(client.send("add", { a: 1, b: 2 })).rejects.toThrow("Add is disabled")
  })

  test("rewords missing IPC send failures", async () => {
    const missingAddress = await getUnusedTcpAddress()
    const client = createNodeClient(missingAddress, schema)

    await expect(client.send("ping")).rejects.toThrow(
      `Could not connect to IPC server at http://${missingAddress.hostname}:${missingAddress.port}/.`,
    )
  })

  test("rewords missing IPC subscribe failures", async () => {
    const missingAddress = await getUnusedTcpAddress()
    const client = createNodeClient(missingAddress, schema)

    await expect(client.subscribe("systemAlert", () => {})).rejects.toThrow(
      `Could not connect to IPC server at http://${missingAddress.hostname}:${missingAddress.port}/.`,
    )
  })

  test("returns generic raw errors for unexpected handler failures", async () => {
    const ipcServer = createServer({
      port: 0,
      schema,
      handlers: {
        ping: () => ({ ok: true as const }),
        echo: ({ text }) => ({ echoed: text }),
        add: () => {
          throw new Error("handler exploded")
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

    await expect(postRaw(address, { name: "add", payload: { a: 1, b: 2 } })).resolves.toEqual({
      statusCode: 500,
      body: JSON.stringify({ error: "Internal server error" }),
    })
  })
})
