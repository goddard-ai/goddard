import { expect, test } from "bun:test"
import { IpcRequestContext, SessionContext } from "../src/context.ts"
import { configureLogging, createLogger } from "../src/logging.ts"

function stripAnsi(value: string): string {
  return value.replace(/\u001B\[[0-9;]*m/g, "")
}

test("pretty logging flattens plain object fields one level", () => {
  const output: string[] = []
  const restoreLogging = configureLogging({
    mode: "pretty",
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
