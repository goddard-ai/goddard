import { expect, test } from "bun:test"

import {
  TerminalCreateRequest,
  TerminalDaemonEvent,
  TerminalEventStreamFilter,
  TerminalResizeRequest,
  TerminalRestartRequest,
} from "../src/daemon/terminals.ts"

test("terminal HTTP request payloads address a connection-local instance", () => {
  expect(
    TerminalCreateRequest.safeParse({
      connectionId: "term-conn-1",
      instanceId: "primary",
      options: {
        cwd: "/repo",
        dimensions: {
          cols: 120,
          rows: 32,
        },
      },
    }).success,
  ).toBe(true)

  expect(
    TerminalRestartRequest.safeParse({
      connectionId: "term-conn-1",
      instanceId: "primary",
    }).success,
  ).toBe(true)
})

test("TerminalResizeRequest requires positive PTY dimensions", () => {
  expect(
    TerminalResizeRequest.safeParse({
      connectionId: "term-conn-1",
      instanceId: "primary",
      dimensions: {
        cols: 0,
        rows: 24,
      },
    }).success,
  ).toBe(false)
})

test("TerminalDaemonEvent accepts lifecycle events and connection-scoped errors", () => {
  expect(
    TerminalDaemonEvent.safeParse({
      type: "terminal.output",
      connectionId: "term-conn-1",
      instanceId: "primary",
      data: "ready\n",
    }).success,
  ).toBe(true)

  expect(
    TerminalDaemonEvent.safeParse({
      type: "terminal.error",
      connectionId: "term-conn-1",
      code: "invalid-request",
      message: "Expected terminal request payload.",
      recoverable: true,
    }).success,
  ).toBe(true)
})

test("terminal stream filters and requests reject empty connection-local ids", () => {
  expect(
    TerminalEventStreamFilter.safeParse({
      connectionId: "",
    }).success,
  ).toBe(false)

  expect(
    TerminalRestartRequest.safeParse({
      connectionId: "term-conn-1",
      instanceId: "",
    }).success,
  ).toBe(false)
})
