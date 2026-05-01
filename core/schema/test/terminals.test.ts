import { expect, test } from "bun:test"

import {
  TerminalClientFrame,
  TerminalDaemonEvent,
  TerminalResizeRequest,
} from "../src/daemon/terminals.ts"

test("TerminalClientFrame accepts the terminal control frames", () => {
  expect(
    TerminalClientFrame.safeParse({
      type: "terminal.create",
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
    TerminalClientFrame.safeParse({
      type: "terminal.input",
      instanceId: "primary",
      data: "\u0003",
    }).success,
  ).toBe(true)

  expect(
    TerminalClientFrame.safeParse({
      type: "terminal.close",
      instanceId: "primary",
    }).success,
  ).toBe(true)
})

test("TerminalResizeRequest requires positive PTY dimensions", () => {
  expect(
    TerminalResizeRequest.safeParse({
      type: "terminal.resize",
      instanceId: "primary",
      dimensions: {
        cols: 0,
        rows: 24,
      },
    }).success,
  ).toBe(false)
})

test("TerminalDaemonEvent accepts lifecycle events and connection-level errors", () => {
  expect(
    TerminalDaemonEvent.safeParse({
      type: "terminal.output",
      instanceId: "primary",
      data: "ready\n",
    }).success,
  ).toBe(true)

  expect(
    TerminalDaemonEvent.safeParse({
      type: "terminal.error",
      code: "invalid-frame",
      message: "Expected terminal client frame.",
      recoverable: true,
    }).success,
  ).toBe(true)
})

test("TerminalClientFrame rejects empty connection-local instance ids", () => {
  expect(
    TerminalClientFrame.safeParse({
      type: "terminal.restart",
      instanceId: "",
    }).success,
  ).toBe(false)
})
