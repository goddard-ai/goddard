import { once } from "node:events"
import type { Server } from "node:http"
import { IpcClientError, type HttpRouteTree } from "@goddard-ai/ipc"
import { createServer } from "@goddard-ai/ipc/node"
import { afterEach, describe, expect, test } from "bun:test"
import { runSafely } from "cmd-ts"

import { daemonIpcRoutes } from "../src/daemon-ipc.ts"
import { createDaemonIpcCommand } from "../src/node/ipc-command.ts"

const cleanups: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanups.length > 0) {
    await cleanups.pop()?.()
  }
})

async function createFixture() {
  const output: string[] = []
  const ipcServer = createServer({
    port: 0,
    routes: daemonIpcRoutes,
    handlers: createHandlers({
      daemon: {
        health: () => ({ ok: true }),
        browserAccess: {
          client: {
            revoke: () => {
              throw new IpcClientError({
                code: "daemon.invalid_request",
                details: { reason: "integration test" },
              })
            },
          },
        },
      },
      session: {
        get: ({ body }: { body: { id: string } }) => ({ id: body.id }),
        streamMessages: () =>
          (async function* () {
            yield { type: "message", message: { role: "assistant", content: "ready" } }
          })(),
      },
    }),
  })

  await once(ipcServer.server, "listening")
  cleanups.push(() => closeServer(ipcServer.server))
  const daemonUrl = readDaemonUrl(ipcServer.server)
  const app = createDaemonIpcCommand({
    env: {
      GODDARD_DAEMON_URL: daemonUrl,
    },
    writeLine: (line) => {
      output.push(line)
    },
  })

  return {
    app,
    daemonUrl,
    output,
  }
}

function readDaemonUrl(server: Server) {
  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("IPC server did not bind to a TCP port")
  }

  return `http://127.0.0.1:${address.port}/`
}

async function closeServer(server: Server) {
  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error)
        return
      }
      resolve()
    })
  })
}

async function getUnusedDaemonUrl() {
  const ipcServer = createServer({
    port: 0,
    routes: daemonIpcRoutes,
    handlers: createHandlers({
      daemon: {
        health: () => ({ ok: true }),
      },
    }),
  })

  await once(ipcServer.server, "listening")
  const daemonUrl = readDaemonUrl(ipcServer.server)
  await closeServer(ipcServer.server)
  return daemonUrl
}

describe("daemon IPC command integration", () => {
  test("prints JSON responses from a real IPC server", async () => {
    const { app, output } = await createFixture()

    const result = await runSafely(app, ["session", "get", "--json", '{"id":"ses_integration"}'])

    expect(result._tag).toBe("ok")
    expect(output).toEqual([JSON.stringify({ id: "ses_integration" })])
  })

  test("prints NDJSON stream events from a real IPC server", async () => {
    const { app, output } = await createFixture()

    const result = await runSafely(app, [
      "session",
      "stream-messages",
      "--json",
      '{"id":"ses_integration"}',
    ])

    expect(result._tag).toBe("ok")
    expect(output).toEqual([
      JSON.stringify({ type: "message", message: { role: "assistant", content: "ready" } }),
    ])
  })

  test("preserves structured client-visible errors from a real IPC server", async () => {
    const { app } = await createFixture()

    await expect(
      runSafely(app, [
        "daemon",
        "browser-access",
        "client",
        "revoke",
        "--json",
        '{"clientId":"client-1"}',
      ]),
    ).rejects.toMatchObject({
      code: "daemon.invalid_request",
      details: { reason: "integration test" },
    })
  })

  test("reports connection failures with the requested daemon URL", async () => {
    const output: string[] = []
    const daemonUrl = await getUnusedDaemonUrl()
    const app = createDaemonIpcCommand({
      env: {
        GODDARD_DAEMON_URL: daemonUrl,
      },
      writeLine: (line) => {
        output.push(line)
      },
    })

    await expect(runSafely(app, ["daemon", "health"])).rejects.toThrow(
      `Could not connect to IPC server at ${daemonUrl}`,
    )
    expect(output).toEqual([])
  })
})

function createHandlers(overrides: Record<string, unknown>) {
  const handlers = createDefaultHandlers(daemonIpcRoutes)
  mergeHandlers(handlers, overrides)
  return handlers as never
}

function createDefaultHandlers(routes: HttpRouteTree): Record<string, unknown> {
  const handlers: Record<string, unknown> = {}

  for (const [key, route] of Object.entries(routes)) {
    handlers[key] =
      route.kind === "resource"
        ? createDefaultHandlers(route.children)
        : () => {
            throw new IpcClientError("Unhandled integration test route")
          }
  }

  return handlers
}

function mergeHandlers(target: Record<string, unknown>, source: Record<string, unknown>) {
  for (const [key, value] of Object.entries(source)) {
    if (
      value &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      typeof target[key] === "object" &&
      target[key] !== null
    ) {
      mergeHandlers(target[key] as Record<string, unknown>, value as Record<string, unknown>)
      continue
    }

    target[key] = value
  }
}
