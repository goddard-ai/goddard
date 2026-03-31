import { once } from "node:events"
import { mkdtemp, rm } from "node:fs/promises"
import { request } from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, describe, expect, test, vi } from "vitest"
import { z } from "zod"
import { $type, type IpcSchema } from "../src/index.ts"
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
          throw new Error("User alerts are disabled for blocked-user")
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
})
