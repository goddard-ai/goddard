import * as assert from "node:assert/strict"
import { test } from "vitest"
import {
  createDaemonIpcClient,
  createDaemonIpcClientFromEnv,
  resolveDaemonConnectionFromEnv,
} from "../src/index.ts"

test("createDaemonIpcClient allows injecting a client factory", () => {
  const calls: Array<{ socketPath: string }> = []
  const client = createDaemonIpcClient({
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    createClient: ({ socketPath }) => {
      calls.push({ socketPath })
      return { kind: "custom" as const, socketPath }
    },
  })

  assert.deepEqual(client, {
    kind: "custom",
    socketPath: "/tmp/daemon.sock",
  })
  assert.deepEqual(calls, [{ socketPath: "/tmp/daemon.sock" }])
})

test("createDaemonIpcClientFromEnv passes the resolved socket path to the injected factory", () => {
  const calls: Array<{ socketPath: string }> = []
  const result = createDaemonIpcClientFromEnv({
    env: {
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
      GODDARD_SESSION_TOKEN: "tok_session",
    },
    createClient: ({ socketPath }) => {
      calls.push({ socketPath })
      return { kind: "custom" as const, socketPath }
    },
  })

  assert.equal(result.daemonUrl, "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock")
  assert.equal(result.sessionToken, "tok_session")
  assert.deepEqual(result.client, {
    kind: "custom",
    socketPath: "/tmp/daemon.sock",
  })
  assert.deepEqual(calls, [{ socketPath: "/tmp/daemon.sock" }])
})

test("resolveDaemonConnectionFromEnv makes env-driven daemon settings explicit", () => {
  const result = resolveDaemonConnectionFromEnv({
    GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    GODDARD_SESSION_TOKEN: "tok_session",
  })

  assert.deepEqual(result, {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    socketPath: "/tmp/daemon.sock",
    sessionToken: "tok_session",
  })
})

test("resolveDaemonConnectionFromEnv can derive the daemon URL from an explicit socket path", () => {
  const result = resolveDaemonConnectionFromEnv({
    GODDARD_DAEMON_SOCKET_PATH: "/tmp/custom-daemon.sock",
    GODDARD_SESSION_TOKEN: "tok_session",
  })

  assert.deepEqual(result, {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fcustom-daemon.sock",
    socketPath: "/tmp/custom-daemon.sock",
    sessionToken: "tok_session",
  })
})
