import { describe, expect, test } from "bun:test"
import { dryRun, runSafely } from "cmd-ts"

import type { DaemonIpcClientFactory } from "../src/node/index.ts"
import { createDaemonIpcCommand } from "../src/node/ipc-command.ts"

type Call = {
  route: string
  payload: unknown
}

function createFixture() {
  const calls: Call[] = []
  const output: string[] = []
  const client = {
    daemon: {
      health: async () => {
        calls.push({ route: "daemon.health", payload: undefined })
        return { ok: true }
      },
      browserAccess: {
        webviewToken: {
          create: async (payload: unknown) => {
            calls.push({ route: "daemon.browserAccess.webviewToken.create", payload })
            return { token: "token-1" }
          },
        },
      },
    },
    adapter: {
      list: async (payload: unknown) => {
        calls.push({ route: "adapter.list", payload })
        return { adapters: [] }
      },
    },
    session: {
      list: async (payload: unknown) => {
        calls.push({ route: "session.list", payload })
        return { sessions: [] }
      },
      get: async (payload: unknown) => {
        calls.push({ route: "session.get", payload })
        return { id: "ses_session_1" }
      },
      async streamMessages(payload: unknown) {
        calls.push({ route: "session.streamMessages", payload })
        return (async function* () {
          yield { message: "one" }
          yield { message: "two" }
        })()
      },
    },
  }
  const createClient: DaemonIpcClientFactory = () => client as never
  const app = createDaemonIpcCommand({
    env: {
      GODDARD_DAEMON_URL: "http://127.0.0.1:1234/",
    },
    createClient,
    writeLine: (line) => {
      output.push(line)
    },
  })

  return {
    app,
    calls,
    output,
  }
}

describe("daemon IPC command", () => {
  test("dispatches no-input routes and prints JSON responses", async () => {
    const { app, calls, output } = createFixture()

    const result = await runSafely(app, ["daemon", "health"])

    expect(result._tag).toBe("ok")
    expect(calls).toEqual([{ route: "daemon.health", payload: undefined }])
    expect(output).toEqual([JSON.stringify({ ok: true })])
  })

  test("validates and dispatches body routes from JSON input", async () => {
    const { app, calls, output } = createFixture()

    const result = await runSafely(app, ["session", "get", "--json", '{"id":"ses_session_1"}'])

    expect(result._tag).toBe("ok")
    expect(calls).toEqual([{ route: "session.get", payload: { id: "ses_session_1" } }])
    expect(output).toEqual([JSON.stringify({ id: "ses_session_1" })])
  })

  test("dispatches string and boolean fields from generated scalar options", async () => {
    const { app, calls, output } = createFixture()

    const result = await runSafely(app, [
      "adapter",
      "list",
      "--cwd",
      "/repo",
      "--include-uninstalled",
      "true",
    ])

    expect(result._tag).toBe("ok")
    expect(calls).toEqual([
      { route: "adapter.list", payload: { cwd: "/repo", includeUninstalled: true } },
    ])
    expect(output).toEqual([JSON.stringify({ adapters: [] })])
  })

  test("dispatches number fields from generated scalar options", async () => {
    const { app, calls, output } = createFixture()

    const result = await runSafely(app, ["session", "list", "--limit", "25", "--cursor", "abc"])

    expect(result._tag).toBe("ok")
    expect(calls).toEqual([{ route: "session.list", payload: { limit: 25, cursor: "abc" } }])
    expect(output).toEqual([JSON.stringify({ sessions: [] })])
  })

  test("prints expected JSON shape when request payload is missing", async () => {
    const { app, calls, output } = createFixture()

    const result = await runSafely(app, ["session", "get"])

    expect(result._tag).toBe("ok")
    expect(calls).toEqual([])
    expect(output).toEqual([JSON.stringify({ id: "<value>" }, null, 2)])
  })

  test("validates and dispatches query routes from JSON input", async () => {
    const { app, calls, output } = createFixture()

    const result = await runSafely(app, [
      "session",
      "stream-messages",
      "--json",
      '{"id":"ses_session_1"}',
    ])

    expect(result._tag).toBe("ok")
    expect(calls).toEqual([{ route: "session.streamMessages", payload: { id: "ses_session_1" } }])
    expect(output).toEqual([JSON.stringify({ message: "one" }), JSON.stringify({ message: "two" })])
  })

  test("uses kebab-case command names for camel-case route keys", async () => {
    const { app, calls, output } = createFixture()

    const result = await runSafely(app, [
      "daemon",
      "browser-access",
      "webview-token",
      "create",
      "--json",
      '{"origin":"https://app.goddardai.org"}',
    ])

    expect(result._tag).toBe("ok")
    expect(calls).toEqual([
      {
        route: "daemon.browserAccess.webviewToken.create",
        payload: { origin: "https://app.goddardai.org" },
      },
    ])
    expect(output).toEqual([JSON.stringify({ token: "token-1" })])
  })

  test("uses route metadata descriptions in generated help", async () => {
    const { app } = createFixture()

    const resourceHelp = await dryRun(app, ["session", "--help"])
    const actionHelp = await dryRun(app, ["session", "list", "--help"])

    expect(resourceHelp._tag).toBe("error")
    expect(actionHelp._tag).toBe("error")
    if (resourceHelp._tag === "error") {
      expect(resourceHelp.error).toContain("Daemon-managed session control.")
      expect(resourceHelp.error).toContain("Lists daemon-managed sessions and pagination state.")
    }
    if (actionHelp._tag === "error") {
      expect(actionHelp.error).toContain("Lists daemon-managed sessions and pagination state.")
    }
  })

  test("rejects invalid JSON before dispatch", async () => {
    const { app, calls } = createFixture()

    const result = await runSafely(app, ["session", "get", "--json", "{"])

    expect(result._tag).toBe("error")
    expect(calls).toEqual([])
  })

  test("rejects schema-invalid JSON before dispatch", async () => {
    const { app, calls } = createFixture()

    const result = await runSafely(app, ["session", "get", "--json", '{"missing":"id"}'])

    expect(result._tag).toBe("error")
    expect(calls).toEqual([])
  })

  test("rejects JSON input for routes without request payloads", async () => {
    const { app, calls } = createFixture()

    const result = await runSafely(app, ["daemon", "health", "--json", "{}"])

    expect(result._tag).toBe("error")
    expect(calls).toEqual([])
  })
})
