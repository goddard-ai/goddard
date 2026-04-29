import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process"
import { createServer } from "node:net"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { afterAll, beforeAll, describe, expect, test } from "bun:test"

const testDir = path.dirname(fileURLToPath(import.meta.url))
const backendDir = path.resolve(testDir, "..")
const startWranglerDevPath = path.resolve(testDir, "start-wrangler-dev.mjs")

let wranglerDev: ChildProcessWithoutNullStreams | undefined
let baseUrl = ""

beforeAll(async () => {
  const port = await getOpenPort()
  baseUrl = `http://127.0.0.1:${port}`
  wranglerDev = spawn("node", [startWranglerDevPath, backendDir, String(port)], {
    cwd: backendDir,
    stdio: "pipe",
  })
  await waitForWranglerReady(wranglerDev)
  await waitForWorker()
}, 30_000)

afterAll(async () => {
  await stopWranglerDev()
}, 30_000)

describe("CloudSession Worker runtime", () => {
  test("routes through the configured Durable Object binding and persists between requests", async () => {
    const sessionId = createTestSessionId("runtime")

    const create = await postTestJson(`/__test/cloud/sessions/${sessionId}/create`, {
      metadata: { provider: "blaxel" },
    })
    const command = await postTestJson(`/__test/cloud/sessions/${sessionId}/commands`, {
      commandId: "cmd_runtime_1",
      type: "initialize",
      payload: { protocol: "acp" },
    })
    const sync = await getTestJson(`/__test/cloud/sessions/${sessionId}/sync?after=0`)

    expect(create.status).toBe(200)
    expect(command.status).toBe(200)
    expect(sync.status).toBe(200)

    const syncBody = (await sync.json()) as {
      cursor: number
      session: { id: string; metadata?: Record<string, unknown> }
      events: Array<{ type: string }>
    }

    expect(syncBody.cursor).toBe(2)
    expect(syncBody.session.id).toBe(sessionId)
    expect(syncBody.session.metadata).toEqual({ provider: "blaxel" })
    expect(syncBody.events.map((event) => event.type)).toEqual([
      "cloud-session.created",
      "cloud-session.command.accepted",
    ])
  })

  test("isolates state by Durable Object id", async () => {
    const firstId = createTestSessionId("first")
    const secondId = createTestSessionId("second")

    await postTestJson(`/__test/cloud/sessions/${firstId}/create`, {})
    await postTestJson(`/__test/cloud/sessions/${firstId}/commands`, {
      commandId: "cmd_first",
      type: "prompt",
      payload: { prompt: "First" },
    })
    await postTestJson(`/__test/cloud/sessions/${secondId}/create`, {})

    const firstSync = await readSync(firstId)
    const secondSync = await readSync(secondId)

    expect(firstSync.session.id).toBe(firstId)
    expect(firstSync.events.map((event) => event.type)).toEqual([
      "cloud-session.created",
      "cloud-session.command.accepted",
    ])
    expect(secondSync.session.id).toBe(secondId)
    expect(secondSync.events.map((event) => event.type)).toEqual(["cloud-session.created"])
  })

  test("exercises the harness WebSocket path through Wrangler", async () => {
    const sessionId = createTestSessionId("harness")
    await postTestJson(`/__test/cloud/sessions/${sessionId}/create`, {})

    const socket = await openTestSocket(`/__test/cloud/sessions/${sessionId}/harness`)
    const hello = await readSocketMessage(socket)
    expect(hello).toMatchObject({
      type: "coordinator.hello",
      harnessEpoch: 1,
      sessionId,
    })

    const commandMessage = readSocketMessage(socket)
    const command = await postTestJson(`/__test/cloud/sessions/${sessionId}/commands`, {
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

    const sync = await readSyncEventually(sessionId, "session/update")
    expect(sync.events.map((event) => event.type)).toContain("session/update")

    socket.close(1000, "done")
    await readSyncEventually(sessionId, "cloud-session.harness.detached")
  })
})

function createTestSessionId(prefix: string) {
  return `cls_${prefix}_${crypto.randomUUID()}`
}

function getWorker() {
  if (!wranglerDev) {
    throw new Error("Worker runtime was not started")
  }

  return wranglerDev
}

async function getTestJson(pathname: string) {
  getWorker()
  return fetch(new URL(pathname, baseUrl))
}

async function postTestJson(pathname: string, body: unknown) {
  getWorker()
  return fetch(new URL(pathname, baseUrl), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  })
}

async function readSync(sessionId: string) {
  const response = await getTestJson(`/__test/cloud/sessions/${sessionId}/sync?after=0`)
  expect(response.status).toBe(200)
  return (await response.json()) as {
    session: { id: string }
    events: Array<{ type: string }>
  }
}

async function readSyncEventually(sessionId: string, eventType: string) {
  for (let attempt = 0; attempt < 10; attempt++) {
    const sync = await readSync(sessionId)
    if (sync.events.some((event) => event.type === eventType)) {
      return sync
    }
    await new Promise((resolve) => setTimeout(resolve, 10))
  }

  throw new Error(`Timed out waiting for ${eventType}`)
}

async function openTestSocket(pathname: string) {
  getWorker()
  const socketUrl = new URL(pathname, baseUrl)
  socketUrl.protocol = socketUrl.protocol === "https:" ? "wss:" : "ws:"

  const socket = new WebSocket(socketUrl)
  await new Promise<void>((resolve, reject) => {
    socket.addEventListener("open", () => resolve(), { once: true })
    socket.addEventListener("error", () => reject(new Error("Harness WebSocket errored")), {
      once: true,
    })
  })

  return socket
}

function getOpenPort() {
  return new Promise<number>((resolve, reject) => {
    const server = createServer()
    server.once("error", reject)
    server.listen(0, "127.0.0.1", () => {
      const address = server.address()
      if (!address || typeof address === "string") {
        server.close()
        reject(new Error("Could not allocate test server port"))
        return
      }

      server.close(() => resolve(address.port))
    })
  })
}

async function waitForWorker() {
  for (let attempt = 0; attempt < 50; attempt++) {
    try {
      const response = await fetch(new URL("/__test/health", baseUrl))
      if (response.status === 204) {
        return
      }
    } catch {
      // Retry until Wrangler's local server has finished binding its port.
    }

    await new Promise((resolve) => setTimeout(resolve, 100))
  }

  throw new Error("Timed out waiting for Wrangler Worker runtime")
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

function waitForWranglerReady(process: ChildProcessWithoutNullStreams) {
  return new Promise<void>((resolve, reject) => {
    let output = ""
    const timeout = setTimeout(() => {
      cleanup()
      reject(new Error(`Timed out waiting for Wrangler dev process: ${output}`))
    }, 30_000)

    const cleanup = () => {
      clearTimeout(timeout)
      process.stdout.off("data", handleStdout)
      process.stderr.off("data", handleStderr)
      process.off("exit", handleExit)
    }
    const handleStdout = (chunk: Buffer) => {
      output += chunk.toString()
      if (output.includes("__GODDARD_WRANGLER_READY__")) {
        cleanup()
        resolve()
      }
    }
    const handleStderr = (chunk: Buffer) => {
      output += chunk.toString()
    }
    const handleExit = (code: number | null) => {
      cleanup()
      reject(new Error(`Wrangler dev process exited before ready: ${code}\n${output}`))
    }

    process.stdout.on("data", handleStdout)
    process.stderr.on("data", handleStderr)
    process.once("exit", handleExit)
  })
}

function stopWranglerDev() {
  return new Promise<void>((resolve) => {
    const process = wranglerDev
    if (!process || process.exitCode !== null) {
      resolve()
      return
    }

    const timeout = setTimeout(() => {
      process.kill("SIGTERM")
      resolve()
    }, 5_000)

    process.once("exit", () => {
      clearTimeout(timeout)
      resolve()
    })
    process.stdin.end("stop\n")
  })
}
