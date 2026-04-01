import { once } from "node:events"
import { mkdtemp, rm } from "node:fs/promises"
import { request } from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, describe, expect, test, vi } from "vitest"
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
      subscription: z.object({
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
  const directory = await mkdtemp(join(tmpdir(), "goddard-ipc-"))
  const socketPath = join(directory, "ipc.sock")
  const ipcServer = createServer(
    socketPath,
    schema,
    {
      ping: () => ({ ok: true }),
      echo: ({ text }) => ({ echoed: text }),
      add: ({ a, b }) => ({ sum: a + b }),
    },
    {
      onSubscribe: ({ name, subscription }) => {
        if (name === "userAlert" && subscription?.userId === "blocked-user") {
          throw new IpcClientError("User alerts are disabled for blocked-user")
        }
      },
    },
  )

  await once(ipcServer.server, "listening")

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
    await rm(directory, { recursive: true, force: true })
  })

  return {
    socketPath,
    client: createNodeClient(socketPath, schema),
    publish: ipcServer.publish,
  }
}

async function postRaw(socketPath: string, body: unknown) {
  const payload = JSON.stringify(body)

  return new Promise<{ statusCode: number; body: string }>((resolve, reject) => {
    const req = request(
      {
        socketPath,
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

describe("core/ipc", () => {
  test("sends validated request/response messages over a unix socket", async () => {
    const { client } = await createFixture()

    await expect(client.send("ping")).resolves.toEqual({ ok: true })
    await expect(client.send("echo", { text: "hello" })).resolves.toEqual({ echoed: "hello" })
    await expect(client.send("add", { a: 2, b: 3 })).resolves.toEqual({ sum: 5 })
  })

  test("rejects invalid request payloads before they cross the process boundary", async () => {
    const { client } = await createFixture()

    await expect(client.send("add", { a: 2, b: "3" } as never)).rejects.toThrow()
  })

  test("returns a structured error for unknown requests", async () => {
    const { socketPath } = await createFixture()

    await expect(postRaw(socketPath, { name: "missing", payload: {} })).resolves.toEqual({
      statusCode: 400,
      body: JSON.stringify({ error: "Unknown request: missing" }),
    })
  })

  test("creates request context and fires request lifecycle hooks", async () => {
    const directory = await mkdtemp(join(tmpdir(), "goddard-ipc-hooks-"))
    const socketPath = join(directory, "ipc.sock")
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
    const ipcServer = createServer(
      socketPath,
      schema,
      {
        ping: (context) => {
          handlerContexts.push(context.traceId)
          return { ok: true }
        },
        echo: ({ text }, context) => {
          handlerContexts.push(context.traceId)
          return { echoed: `${text}:${context.traceId}` }
        },
        add: ({ a, b }, context) => {
          handlerContexts.push(context.traceId)
          return { sum: a + b }
        },
      },
      {
        createRequestContext: ({ name }) => ({
          traceId: `${name}-${String(++requestCount)}`,
        }),
        onRequestReceived: ({ name, payload, context }) => {
          received.push({ name, payload, traceId: context.traceId })
        },
        onResponseSent: ({ name, payload, response, durationMs, context }) => {
          responded.push({ name, payload, response, durationMs, traceId: context.traceId })
        },
      },
    )

    await once(ipcServer.server, "listening")
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
      await rm(directory, { recursive: true, force: true })
    })

    const client = createNodeClient(socketPath, schema)
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

  test("applies stream subscription filters on the server side", async () => {
    const { client, publish } = await createFixture()
    const onMessage = vi.fn()

    const unsubscribe = await client.subscribe("userAlert", { userId: "user-1" }, onMessage)
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

  test("rejects stream subscriptions when the server-side validator fails", async () => {
    const { client } = await createFixture()

    await expect(
      client.subscribe("userAlert", { userId: "blocked-user" }, () => {}),
    ).rejects.toThrow("User alerts are disabled for blocked-user")
  })

  test("fires request-failed hooks when a handler throws", async () => {
    const directory = await mkdtemp(join(tmpdir(), "goddard-ipc-errors-"))
    const socketPath = join(directory, "ipc.sock")
    const failures: Array<{
      name: string
      payload: unknown
      errorMessage: string
      durationMs: number
      traceId: string
    }> = []
    const ipcServer = createServer(
      socketPath,
      schema,
      {
        ping: () => ({ ok: true }),
        echo: ({ text }) => ({ echoed: text }),
        add: () => {
          throw new Error("handler exploded")
        },
      },
      {
        createRequestContext: () => ({ traceId: "trace-add" }),
        onRequestFailed: ({ name, payload, error, durationMs, context }) => {
          failures.push({
            name,
            payload,
            errorMessage: error instanceof Error ? error.message : String(error),
            durationMs,
            traceId: context.traceId,
          })
        },
      },
    )

    await once(ipcServer.server, "listening")
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
      await rm(directory, { recursive: true, force: true })
    })

    const client = createNodeClient(socketPath, schema)
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
    const directory = await mkdtemp(join(tmpdir(), "goddard-ipc-client-errors-"))
    const socketPath = join(directory, "ipc.sock")
    const ipcServer = createServer(socketPath, schema, {
      ping: () => ({ ok: true }),
      echo: ({ text }) => ({ echoed: text }),
      add: () => {
        throw new IpcClientError("Add is disabled")
      },
    })

    await once(ipcServer.server, "listening")
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
      await rm(directory, { recursive: true, force: true })
    })

    const client = createNodeClient(socketPath, schema)
    await expect(client.send("add", { a: 1, b: 2 })).rejects.toThrow("Add is disabled")
  })

  test("returns generic raw errors for unexpected handler failures", async () => {
    const directory = await mkdtemp(join(tmpdir(), "goddard-ipc-raw-errors-"))
    const socketPath = join(directory, "ipc.sock")
    const ipcServer = createServer(socketPath, schema, {
      ping: () => ({ ok: true }),
      echo: ({ text }) => ({ echoed: text }),
      add: () => {
        throw new Error("handler exploded")
      },
    })

    await once(ipcServer.server, "listening")
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
      await rm(directory, { recursive: true, force: true })
    })

    await expect(postRaw(socketPath, { name: "add", payload: { a: 1, b: 2 } })).resolves.toEqual({
      statusCode: 500,
      body: JSON.stringify({ error: "Internal server error" }),
    })
  })
})
