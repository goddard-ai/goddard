import * as pty from "node-pty"

import type { SessionDriver } from "./types.ts"

export interface PtyServerDriverOptions {
  command?: string
  args?: string[]
  cwd?: string
}

export function createPtyServerDriver(options: PtyServerDriverOptions): SessionDriver {
  const command = options.command || "bash"
  const args = options.args || []
  const cwd = options.cwd || process.cwd()

  const ptyProcess = pty.spawn(command, args, {
    name: "xterm-color",
    cols: 80,
    rows: 24,
    cwd,
    env: process.env as Record<string, string>,
  })

  return {
    name: "pty",
    start: (input) => {
      if (input.resume) {
        throw new Error("pty does not support resume startup input")
      }
    },
    sendEvent: (event) => {
      if (event.type === "input.terminal") {
        ptyProcess.write(event.data)
        return
      }
      if (event.type === "input.text") {
        ptyProcess.write(event.text)
        return
      }
      if (event.type === "terminal.resize") {
        const cols = Math.max(1, Math.floor(event.cols))
        const rows = Math.max(1, Math.floor(event.rows))
        ptyProcess.resize(cols, rows)
      }
    },
    onEvent: (listener) => {
      const outputDisposable = ptyProcess.onData((data) => {
        listener({ type: "output.terminal", data })
      })
      const exitDisposable = ptyProcess.onExit(({ exitCode }) => {
        listener({ type: "session.exit", exitCode })
      })

      return () => {
        outputDisposable.dispose()
        exitDisposable.dispose()
      }
    },
    getCapabilities: () => ({
      terminal: {
        enabled: true,
        canResize: true,
        hasScreenState: true,
      },
      normalizedOutput: true,
    }),
    close: () => {
      ptyProcess.kill()
    },
  }
}

export const driver: SessionDriver = createPtyServerDriver({})
