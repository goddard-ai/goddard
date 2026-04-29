import { listDurableObjectIds, reset, runInDurableObject } from "cloudflare:test"
import { env } from "cloudflare:workers"
import { afterEach, describe, expect, test } from "vitest"

import { CloudSession } from "../src/worker.ts"

afterEach(async () => {
  await reset()
})

describe("CloudSession Durable Object in the Workers runtime", () => {
  test("uses the configured Durable Object binding and persists coordinator storage", async () => {
    const stub = getCloudSessionStub("runtime-storage")

    const create = await fetchCloudSession(stub, "/create", {
      sessionId: "cls_runtime_storage",
      metadata: { provider: "blaxel" },
    })
    const command = await fetchCloudSession(stub, "/commands", {
      commandId: "cmd_runtime_1",
      type: "initialize",
      payload: { protocol: "acp" },
    })
    const sync = await fetchCloudSession(stub, "/sync?after=0")

    expect(create.status).toBe(200)
    expect(command.status).toBe(200)
    expect(sync.status).toBe(200)
    const syncBody = (await sync.json()) as {
      cursor: number
      session: { id: string; metadata?: Record<string, unknown> }
      events: Array<{ type: string }>
    }

    expect(syncBody.cursor).toBe(2)
    expect(syncBody.session.id).toBe("cls_runtime_storage")
    expect(syncBody.session.metadata).toEqual({ provider: "blaxel" })
    expect(syncBody.events.map((event) => event.type)).toEqual([
      "cloud-session.created",
      "cloud-session.command.accepted",
    ])

    const ids = await listDurableObjectIds(env.CLOUD_SESSION)
    expect(ids).toHaveLength(1)

    const persisted = await runInDurableObject(
      stub as DurableObjectStub,
      async (instance, state) => {
        expect(instance).toBeInstanceOf(CloudSession)
        return (await state.storage.get("cloud-session-state")) as {
          session: { id: string }
          events: Array<{ type: string }>
        }
      },
    )

    expect(persisted.session.id).toBe("cls_runtime_storage")
    expect(persisted.events.map((event) => event.type)).toEqual([
      "cloud-session.created",
      "cloud-session.command.accepted",
    ])
  })

  test("isolates sessions by Durable Object id", async () => {
    const first = getCloudSessionStub("first")
    const second = getCloudSessionStub("second")

    await fetchCloudSession(first, "/create", { sessionId: "cls_first" })
    await fetchCloudSession(first, "/commands", {
      commandId: "cmd_first",
      type: "prompt",
      payload: { prompt: "First" },
    })
    await fetchCloudSession(second, "/create", { sessionId: "cls_second" })

    const firstSync = await readSync(first)
    const secondSync = await readSync(second)

    expect(firstSync.session.id).toBe("cls_first")
    expect(firstSync.events.map((event) => event.type)).toEqual([
      "cloud-session.created",
      "cloud-session.command.accepted",
    ])
    expect(secondSync.session.id).toBe("cls_second")
    expect(secondSync.events.map((event) => event.type)).toEqual(["cloud-session.created"])
  })

  test("exercises the harness WebSocket channel with Cloudflare WebSocketPair", async () => {
    const stub = getCloudSessionStub("harness")
    await fetchCloudSession(stub, "/create", { sessionId: "cls_harness_runtime" })

    const harnessResponse = await stub.fetch("https://cloud-session.internal/harness", {
      headers: { upgrade: "websocket" },
    })

    expect(harnessResponse.status).toBe(101)
    const socket = harnessResponse.webSocket
    expect(socket).toBeDefined()
    if (!socket) {
      throw new Error("Expected harness WebSocket")
    }

    const helloMessage = readSocketMessage(socket)
    socket.accept()
    const hello = await helloMessage
    expect(hello).toMatchObject({
      type: "coordinator.hello",
      harnessEpoch: 1,
      sessionId: "cls_harness_runtime",
    })

    const commandMessage = readSocketMessage(socket)
    const command = await fetchCloudSession(stub, "/commands", {
      commandId: "cmd_harness_runtime",
      type: "prompt",
      payload: { prompt: "Continue" },
    })

    expect(command.status).toBe(200)
    expect(await commandMessage).toMatchObject({
      type: "command",
      harnessEpoch: 1,
      command: { commandId: "cmd_harness_runtime" },
    })

    socket.send(
      JSON.stringify({
        type: "event",
        eventType: "session/update",
        payload: { update: "working" },
      }),
    )

    const sync = await readSyncEventually(stub, "session/update")
    expect(sync.events.map((event) => event.type)).toContain("session/update")

    socket.close(1000, "done")
    await readSyncEventually(stub, "cloud-session.harness.detached")
  })
})

function getCloudSessionStub(name: string) {
  return env.CLOUD_SESSION.get(env.CLOUD_SESSION.idFromName(`cloud-session-test:${name}`))
}

function fetchCloudSession(stub: DurableObjectStub, pathname: string, body?: unknown) {
  return stub.fetch(
    new Request(`https://cloud-session.internal${pathname}`, {
      method: body === undefined ? "GET" : "POST",
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    }),
  )
}

async function readSync(stub: DurableObjectStub) {
  const response = await fetchCloudSession(stub, "/sync?after=0")
  expect(response.status).toBe(200)
  return (await response.json()) as {
    session: { id: string }
    events: Array<{ type: string }>
  }
}

async function readSyncEventually(stub: DurableObjectStub, eventType: string) {
  for (let attempt = 0; attempt < 10; attempt++) {
    const sync = await readSync(stub)
    if (sync.events.some((event) => event.type === eventType)) {
      return sync
    }
    await new Promise((resolve) => setTimeout(resolve, 10))
  }

  throw new Error(`Timed out waiting for ${eventType}`)
}

function readSocketMessage(socket: WebSocket) {
  return new Promise<Record<string, unknown>>((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error("Timed out waiting for harness WebSocket message"))
    }, 1_000)

    socket.addEventListener(
      "message",
      (event) => {
        clearTimeout(timeout)
        resolve(JSON.parse(String(event.data)) as Record<string, unknown>)
      },
      { once: true },
    )
    socket.addEventListener(
      "error",
      () => {
        clearTimeout(timeout)
        reject(new Error("Harness WebSocket errored"))
      },
      { once: true },
    )
  })
}
