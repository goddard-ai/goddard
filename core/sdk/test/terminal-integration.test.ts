import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { startDaemonServer } from "@goddard-ai/daemon/ipc"
import type { IpcClientHookEvent } from "@goddard-ai/ipc"
import { expect, test } from "bun:test"

import { GoddardSdk } from "../src/index.ts"

function createBackendClient() {
  return {
    auth: {
      device: {
        start: async () => {
          throw new Error("unused")
        },
        complete: async () => {
          throw new Error("unused")
        },
      },
      session: {
        current: async () => {
          throw new Error("unused")
        },
      },
    },
    pullRequests: {
      create: async () => {
        throw new Error("unused")
      },
      managed: async () => ({ managed: true }),
      comments: {
        create: async () => ({ success: true }),
      },
    },
    webhooks: {
      github: async () => ({ type: "noop" }),
    },
    repositories: {
      stream: async () => new Response(),
    },
    stream: {
      subscribe: async () => {
        throw new Error("unused")
      },
    },
  }
}

async function waitFor(check: () => boolean, timeoutMs = 3000) {
  const startedAt = Date.now()
  while (Date.now() - startedAt < timeoutMs) {
    if (check()) {
      return
    }
    await Bun.sleep(25)
  }
  throw new Error("Timed out waiting for condition")
}

test("SDK terminal connections serialize HTTP controls", async () => {
  const server = await startDaemonServer(createBackendClient() as never, { port: 0 })
  const writeLifecycle: IpcClientHookEvent["type"][] = []
  const client = createDaemonIpcClient({
    daemonUrl: server.daemonUrl,
    ipcHook(event) {
      if (event.routeName === "terminal.write") {
        writeLifecycle.push(event.type)
      }
    },
  })
  const terminal = await new GoddardSdk({ client }).terminal.connect()
  let stop: (() => Promise<void>) | null = null

  try {
    stop = await terminal.subscribe(
      () => {},
      () => {},
    )
    await terminal.create({ instanceId: "primary", options: { command: "/bin/cat" } })

    await Promise.all([
      terminal.write({ instanceId: "primary", data: "first\n" }),
      terminal.write({ instanceId: "primary", data: "second\n" }),
    ])

    expect(writeLifecycle).toEqual([
      "request.start",
      "request.success",
      "request.start",
      "request.success",
    ])
  } finally {
    await stop?.()
    await server.close()
  }
})

test("SDK terminal subscriptions report unexpected stream failure", async () => {
  const server = await startDaemonServer(createBackendClient() as never, { port: 0 })
  const client = createDaemonIpcClient({ daemonUrl: server.daemonUrl })
  const terminal = await new GoddardSdk({ client }).terminal.connect()
  const streamFailure = new Error("output handler failed")
  let endedWith: unknown

  try {
    await terminal.subscribe(
      async () => {
        throw streamFailure
      },
      (error) => {
        endedWith = error
      },
    )
    await terminal.create({ instanceId: "primary", options: { command: "/bin/cat" } })

    await waitFor(() => endedWith !== undefined)
    expect(endedWith).toBe(streamFailure)
    await expect(
      terminal.write({ instanceId: "primary", data: "after stream failure\n" }),
    ).rejects.toThrow()
  } finally {
    await server.close()
  }
})
