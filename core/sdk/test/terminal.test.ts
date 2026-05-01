import { expect, test, vi } from "bun:test"

import type { GoddardTerminalConnection } from "../src/index.ts"

test("GoddardTerminalConnection models connection-local terminal controls", async () => {
  const onEvent = vi.fn()
  const sent: string[] = []
  const terminal: GoddardTerminalConnection = {
    create: async (input) => {
      sent.push(`create:${input.instanceId}`)
    },
    write: async (input) => {
      sent.push(`write:${input.instanceId}:${input.data}`)
    },
    resize: async (input) => {
      sent.push(`resize:${input.instanceId}:${input.dimensions.cols}x${input.dimensions.rows}`)
    },
    restart: async (input) => {
      sent.push(`restart:${input.instanceId}:${input.options?.cwd ?? ""}`)
    },
    close: async (input) => {
      sent.push(`close:${input.instanceId}`)
    },
    disconnect: async () => {
      sent.push("disconnect")
    },
    onEvent: (handler) => {
      handler({
        type: "terminal.output",
        instanceId: "primary",
        data: "ok\n",
      })
      return () => {
        sent.push("unsubscribe")
      }
    },
  }

  const unsubscribe = terminal.onEvent(onEvent)
  await terminal.create({ instanceId: "primary", options: { cwd: "/repo" } })
  await terminal.write({ instanceId: "primary", data: "ls\n" })
  await terminal.resize({ instanceId: "primary", dimensions: { cols: 100, rows: 30 } })
  await terminal.restart({ instanceId: "primary", options: { cwd: "/repo" } })
  await terminal.close({ instanceId: "primary" })
  await terminal.disconnect()
  unsubscribe()

  expect(onEvent).toHaveBeenCalledWith({
    type: "terminal.output",
    instanceId: "primary",
    data: "ok\n",
  })
  expect(sent).toEqual([
    "create:primary",
    "write:primary:ls\n",
    "resize:primary:100x30",
    "restart:primary:/repo",
    "close:primary",
    "disconnect",
    "unsubscribe",
  ])
})
