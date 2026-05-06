import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import type { TerminalDaemonEvent } from "@goddard-ai/schema/daemon/terminals"
import { expect, test } from "bun:test"

import type { BackendClient } from "../src/backend.ts"
import { startDaemonServer } from "../src/ipc.ts"
import { send, subscribe } from "./ipc-client-helpers.ts"

function createBackendClient(): BackendClient {
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
  } as unknown as BackendClient
}

async function waitFor(check: () => boolean | Promise<boolean>, timeoutMs = 3000) {
  const startedAt = Date.now()
  while (Date.now() - startedAt < timeoutMs) {
    if (await check()) {
      return
    }
    await Bun.sleep(25)
  }
  throw new Error("Timed out waiting for condition")
}

test("daemon terminal IPC owns connection-local instances and disposes them on stream close", async () => {
  const server = await startDaemonServer(createBackendClient(), { port: 0 })
  const client = createDaemonIpcClient({ daemonUrl: server.daemonUrl })
  let unsubscribe: (() => void) | null = null

  try {
    const { connectionId } = await send(client, "terminal.connect", {})
    const events: TerminalDaemonEvent[] = []

    await expect(
      subscribe(client, { name: "terminal.event", filter: { connectionId: "missing" } }, () => {}),
    ).rejects.toThrow("Terminal connection missing is not active.")

    unsubscribe = await subscribe(
      client,
      { name: "terminal.event", filter: { connectionId } },
      (event) => {
        events.push(event)
      },
    )

    const first = await send(client, "terminal.create", {
      connectionId,
      instanceId: "primary",
      options: {
        command: "/bin/cat",
        dimensions: { cols: 80, rows: 24 },
      },
    })
    const second = await send(client, "terminal.create", {
      connectionId,
      instanceId: "secondary",
      options: {
        command: "/bin/cat",
        dimensions: { cols: 100, rows: 30 },
      },
    })

    expect(first.terminal.instanceId).toBe("primary")
    expect(second.terminal.instanceId).toBe("secondary")
    await waitFor(() => events.filter((event) => event.type === "terminal.created").length === 2)

    await expect(
      send(client, "terminal.create", {
        connectionId,
        instanceId: "primary",
        options: { command: "/bin/cat" },
      }),
    ).rejects.toThrow("Terminal instance primary already exists on this connection.")
    await expect(
      send(client, "terminal.write", {
        connectionId,
        instanceId: "missing",
        data: "ignored\n",
      }),
    ).rejects.toThrow("Terminal instance missing does not exist on this connection.")
    await expect(
      send(client, "terminal.write", {
        connectionId: "other",
        instanceId: "primary",
        data: "ignored\n",
      }),
    ).rejects.toThrow("Terminal connection other is not active.")

    await send(client, "terminal.write", {
      connectionId,
      instanceId: "secondary",
      data: "hello from secondary\n",
    })
    await waitFor(() =>
      events.some(
        (event) =>
          event.type === "terminal.output" &&
          event.instanceId === "secondary" &&
          event.data.includes("hello from secondary"),
      ),
    )

    unsubscribe()
    unsubscribe = null
    await waitFor(async () => {
      try {
        await send(client, "terminal.write", {
          connectionId,
          instanceId: "secondary",
          data: "after close\n",
        })
        return false
      } catch (error) {
        return error instanceof Error && error.message.includes("is not active")
      }
    })
  } finally {
    unsubscribe?.()
    await server.close()
  }
})
