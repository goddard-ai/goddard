import { expect, test } from "vitest"
import { createDaemonIpcClient } from "../src/index.ts"
import { createDaemonIpcClientFromEnv, resolveDaemonConnectionFromEnv } from "../src/node/index.ts"

test("createDaemonIpcClient allows injecting a client factory", () => {
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

test("createDaemonIpcClient passes explicit TCP daemon URLs to the injected factory", () => {
  const calls: Array<{ type: string; host: string; port: number }> = []
  const client = createDaemonIpcClient({
    daemonUrl: "http://127.0.0.1:7777/",
    createClient: (connection) => {
      if (connection.type !== "tcp") {
        throw new Error("Expected a TCP connection")
      }
      calls.push({ type: connection.type, host: connection.host, port: connection.port })
      return { kind: "custom" as const, host: connection.host, port: connection.port }
    },
  })

  expect(client).toEqual({
    kind: "custom",
    host: "127.0.0.1",
    port: 7777,
  })
  expect(calls).toEqual([{ type: "tcp", host: "127.0.0.1", port: 7777 }])
})

test("resolveDaemonConnectionFromEnv makes env-driven daemon settings explicit", () => {
  const result = resolveDaemonConnectionFromEnv({
    GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
  })

  expect(result).toEqual({
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    daemonConnection: {
      type: "socket",
      socketPath: "/tmp/daemon.sock",
    },
  })
})

test("resolveDaemonConnectionFromEnv can derive the daemon URL from an explicit socket path", () => {
  const result = resolveDaemonConnectionFromEnv({
    GODDARD_DAEMON_SOCKET_PATH: "/tmp/custom-daemon.sock",
  })

  expect(result).toEqual({
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fcustom-daemon.sock",
    daemonConnection: {
      type: "socket",
      socketPath: "/tmp/custom-daemon.sock",
    },
  })
})

test("resolveDaemonConnectionFromEnv supports explicit TCP daemon URLs", () => {
  const result = resolveDaemonConnectionFromEnv({
    GODDARD_DAEMON_URL: "http://127.0.0.1:7777/",
  })

  expect(result).toEqual({
    daemonUrl: "http://127.0.0.1:7777/",
    daemonConnection: {
      type: "tcp",
      host: "127.0.0.1",
      port: 7777,
    },
  })
})
