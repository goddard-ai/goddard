import * as pty from "node-pty"

import { SessionDriver } from "./types.ts"

export interface PtyServerDriverOptions {
  command?: string
  args?: string[]
  cwd?: string
}

export default class PtyDriver extends SessionDriver {
  readonly name = "pty" as const
  private ptyProcess: pty.IPty | undefined
  private outputDisposable?: { dispose: () => void }
  private exitDisposable?: { dispose: () => void }
  private readonly options: PtyServerDriverOptions

  constructor(options: PtyServerDriverOptions = {}) {
    super()
    this.options = options
  }

  start(input: Parameters<SessionDriver["start"]>[0]) {
    if (input.resume) {
      throw new Error("pty does not support resume startup input")
    }

    const command = this.options.command || "bash"
    const args = this.options.args || []
    const cwd = this.options.cwd || process.cwd()

    this.ptyProcess = pty.spawn(command, args, {
      name: "xterm-color",
      cols: 80,
      rows: 24,
      cwd,
      env: process.env as Record<string, string>,
    })
    
    this.outputDisposable = this.ptyProcess.onData((data) => {
      this.emit({ type: "output.terminal", data })
    })
    
    this.exitDisposable = this.ptyProcess.onExit(({ exitCode }) => {
      this.emit({ type: "session.exit", exitCode })
    })
  }

  sendEvent(event: Parameters<SessionDriver["sendEvent"]>[0]) {
    if (!this.ptyProcess) return

    if (event.type === "input.terminal") {
      this.ptyProcess.write(event.data)
      return
    }
    if (event.type === "input.text") {
      this.ptyProcess.write(event.text)
      return
    }
    if (event.type === "terminal.resize") {
      const cols = Math.max(1, Math.floor(event.cols))
      const rows = Math.max(1, Math.floor(event.rows))
      this.ptyProcess.resize(cols, rows)
    }
  }

  getCapabilities() {
    return {
      terminal: {
        enabled: true,
        canResize: true,
        hasScreenState: true,
      },
      normalizedOutput: true,
    }
  }

  close() {
    this.outputDisposable?.dispose()
    this.exitDisposable?.dispose()
    this.ptyProcess?.kill()
  }
}

export function createPtyServerDriver(options: PtyServerDriverOptions): SessionDriver {
  return new PtyDriver(options)
}
