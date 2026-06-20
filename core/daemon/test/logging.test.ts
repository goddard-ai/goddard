import { createLogStore } from "@goddard-ai/logs"
import { expect, test } from "bun:test"

import { IpcRequestContext, SessionContext } from "../src/context.ts"
import { configureLogging, createDebug, createLogger } from "../src/logging.ts"

const ansiColorPattern = new RegExp(String.raw`\u001B\[[0-9;]*m`, "g")

function stripAnsi(value: string): string {
  return value.replace(ansiColorPattern, "")
}

test("compact logging flattens plain object fields one level", () => {
  const output: string[] = []
  const restoreLogging = configureLogging({
    mode: "compact",
    writeLine: (line) => {
      output.push(line)
    },
  })

  try {
    createLogger().log("example.event", {
      nested: {
        count: 2,
        detail: {
          depth: 3,
        },
        skipped: undefined,
      },
      empty: {},
      active: true,
    })
  } finally {
    restoreLogging()
  }

  expect(output).toHaveLength(1)

  const line = stripAnsi(output[0] ?? "")
  expect(line).toContain("active=true")
  expect(line).toContain("nested.count=2")
  expect(line).toContain("nested.detail={ depth: 3 }")
  expect(line).toContain("empty={}")
  expect(line).not.toContain("nested={")
  expect(line).not.toContain("nested.skipped=")
})

test("json logging preserves null-valued daemon context fields", () => {
  const output: string[] = []
  const restoreLogging = configureLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
  })

  try {
    IpcRequestContext.run(
      {
        opId: "op-1",
        sessionId: null,
        setSessionId: () => {},
      },
      () =>
        SessionContext.run(
          {
            sessionId: "ses_123",
            acpSessionId: null,
            cwd: "/workspace",
            repository: null,
            prNumber: null,
            worktreeDir: null,
            worktreePoweredBy: null,
          },
          () => {
            createLogger().log("example.context")
          },
        ),
    )
  } finally {
    restoreLogging()
  }

  expect(output).toHaveLength(1)

  const entry = JSON.parse(output[0] ?? "") as Record<string, unknown>
  expect(entry.ipcRequest).toEqual({
    opId: "op-1",
    sessionId: null,
  })
  expect(entry.session).toEqual({
    sessionId: "ses_123",
    acpSessionId: null,
    cwd: "/workspace",
    repository: null,
    prNumber: null,
    worktreeDir: null,
    worktreePoweredBy: null,
  })
})

test("snapshot logger preserves captured async context outside the original run", () => {
  const output: string[] = []
  const restoreLogging = configureLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
  })

  try {
    const logger = IpcRequestContext.run(
      {
        opId: "op-1",
        sessionId: null,
        setSessionId: () => {},
      },
      () =>
        SessionContext.run(
          {
            sessionId: "ses_456",
            acpSessionId: "acp_456",
            cwd: "/snapshot-workspace",
            repository: "acme/repo",
            prNumber: 12,
            worktreeDir: null,
            worktreePoweredBy: null,
          },
          () => createLogger().snapshot(),
        ),
    )

    logger.log("example.snapshot")
  } finally {
    restoreLogging()
  }

  expect(output).toHaveLength(1)

  const entry = JSON.parse(output[0] ?? "") as Record<string, unknown>
  expect(entry.ipcRequest).toEqual({
    opId: "op-1",
    sessionId: null,
  })
  expect(entry.session).toEqual({
    sessionId: "ses_456",
    acpSessionId: "acp_456",
    cwd: "/snapshot-workspace",
    repository: "acme/repo",
    prNumber: 12,
    worktreeDir: null,
    worktreePoweredBy: null,
  })
})

test("debug logger writes scoped durable rows without terminal output", () => {
  const output: string[] = []
  const store = createLogStore({ databasePath: ":memory:" })
  const restoreLogging = configureLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
    store,
  })

  try {
    IpcRequestContext.run(
      {
        opId: "op-1",
        sessionId: null,
        setSessionId: () => {},
      },
      () => {
        createDebug("ipc.server")("ipc.request_received", {
          requestName: "session.history",
        })
      },
    )

    expect(output).toHaveLength(0)
    expect(store.query({ debugScope: "ipc" })).toEqual([
      expect.objectContaining({
        scope: "daemon",
        level: "debug",
        message: "ipc.request_received",
        properties: {
          debugScope: "ipc.server",
          ipcRequest: {
            opId: "op-1",
            sessionId: null,
          },
          requestName: "session.history",
        },
      }),
    ])
  } finally {
    restoreLogging()
    store.close()
  }
})
