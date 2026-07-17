import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import type { TerminalDaemonEvent } from "@goddard-ai/schema/daemon/terminals"
import { expect, test } from "bun:test"

import type { BackendClient } from "../src/backend.ts"
import { createDaemonRuntime, startDaemonServer } from "../src/ipc.ts"

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
  const runtime = await createDaemonRuntime({
    backendClient: createBackendClient(),
    port: 0,
  })
  const server = await startDaemonServer(runtime)
  const client = createDaemonIpcClient({ daemonUrl: server.daemonUrl })
  const streamController = new AbortController()
  let consumeEvents: Promise<void> | null = null

  try {
    const { connectionId } = await client.terminal.connect({})
    const events: TerminalDaemonEvent[] = []

    const eventStream = await client.terminal.event(
      { connectionId },
      { signal: streamController.signal },
    )
    consumeEvents = (async () => {
      try {
        for await (const event of eventStream) {
          events.push(event)
        }
      } catch (error) {
        if (!streamController.signal.aborted) {
          throw error
        }
      }
    })()

    const first = await client.terminal.create({
      connectionId,
      instanceId: "primary",
      options: {
        command: "/bin/cat",
        dimensions: { cols: 80, rows: 24 },
      },
    })
    const second = await client.terminal.create({
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

    await client.terminal.write({
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

    streamController.abort()
    await consumeEvents
    consumeEvents = null
    await waitFor(async () => {
      try {
        await client.terminal.write({
          connectionId,
          instanceId: "secondary",
          data: "after close\n",
        })
        return false
      } catch (error) {
        return error instanceof Error
      }
    })
  } finally {
    streamController.abort()
    await consumeEvents?.catch(() => {})
    await server.close()
    await runtime.close()
  }
})
