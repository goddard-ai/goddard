import { expect, test } from "bun:test"
import { createDaemonIpcClient, createDaemonIpcClientFromEnv } from "../src/node/index.ts"

test("createDaemonIpcClient passes the explicit socket path to the injected factory", () => {
  const calls: Array<{ socketPath: string }> = []
  const client = createDaemonIpcClient({
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    createClient: ({ socketPath }) => {
      calls.push({ socketPath })
      return { kind: "custom" as const, socketPath }
    },
  })

  expect(client).toEqual({
    kind: "custom",
    socketPath: "/tmp/daemon.sock",
  })
  expect(calls).toEqual([{ socketPath: "/tmp/daemon.sock" }])
})

test("createDaemonIpcClientFromEnv passes the resolved socket path to the injected factory", () => {
  const calls: Array<{ socketPath: string }> = []
  const result = createDaemonIpcClientFromEnv({
    env: {
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    },
    createClient: ({ socketPath }) => {
      calls.push({ socketPath })
      return { kind: "custom" as const, socketPath }
    },
  })

  expect(result.daemonUrl).toBe("http://unix/?socketPath=%2Ftmp%2Fdaemon.sock")
  expect(result.client).toEqual({
    kind: "custom",
    socketPath: "/tmp/daemon.sock",
  })
  expect(calls).toEqual([{ socketPath: "/tmp/daemon.sock" }])
})

test("createDaemonIpcClientFromEnv can derive the daemon URL from an explicit socket path", () => {
  const result = createDaemonIpcClientFromEnv({
    env: {
      GODDARD_DAEMON_SOCKET_PATH: "/tmp/custom-daemon.sock",
    },
    createClient: ({ socketPath }) => ({ kind: "custom" as const, socketPath }),
  })

  expect(result).toEqual({
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fcustom-daemon.sock",
    client: {
      kind: "custom",
      socketPath: "/tmp/custom-daemon.sock",
    },
  })
})
