import { spawn } from "node:child_process"
import type { ChildProcessByStdio } from "node:child_process"
import { Readable } from "node:stream"

import {
  sessionServerEventSchema,
  type SessionClientEvent,
  type SessionServerEvent,
  type SessionStartupInput,
} from "@goddard-ai/session-protocol"

import { SessionDriver as BaseSessionDriver, type SessionDriverName } from "./types.ts"

type ChildProcess = ChildProcessByStdio<null, Readable, Readable>

export namespace JsonLineSubprocess {
  export interface DriverOptions {
    name: SessionDriverName
    command: string
    cwd?: string
  }

  export interface HandlerContext {
    emit: (event: SessionServerEvent) => void
    getSessionId: () => string | undefined
    setSessionId: (id: string) => void
  }

  export abstract class SessionDriver extends BaseSessionDriver {
    readonly name: SessionDriverName
    protected readonly command: string
    protected readonly cwd?: string

    private sessionId: string | undefined
    private activeChild: ChildProcess | undefined
    private closed = false
    private queue = Promise.resolve()

    constructor(options: DriverOptions) {
      super()
      this.name = options.name
      this.command = options.command
      this.cwd = options.cwd
    }

    protected abstract buildArgs(text: string, sessionId?: string): string[]

    protected abstract handleJsonLine(payload: unknown, context: HandlerContext): void

    start(input: SessionStartupInput) {
      this.sessionId = input.resume
    }

    async sendEvent(event: SessionClientEvent) {
      if (event.type !== "input.text") {
        throw new Error(`${this.name} only supports input.text events`)
      }

      if (this.activeChild) {
        this.activeChild.kill()
        this.activeChild = undefined
      }

      const p = this.runTurn(event.text)
      this.queue = p
      await p
    }

    getCapabilities() {
      return {
        terminal: {
          enabled: false,
          canResize: false,
          hasScreenState: false,
        },
        normalizedOutput: true,
      }
    }

    close() {
      this.closed = true
      this.activeChild?.kill()
      this.activeChild = undefined
    }

    private async runTurn(text: string) {
      if (this.closed) {
        throw new Error(`${this.name} driver is closed`)
      }

      const child = spawn(this.command, this.buildArgs(text, this.sessionId), {
        stdio: ["ignore", "pipe", "pipe"],
        cwd: this.cwd,
        env: process.env,
      })
      this.activeChild = child

      let stdoutBuffer = ""
      let stderrBuffer = ""

      const flushStdoutLine = (line: string) => {
        const trimmed = line.trim()
        if (trimmed.length === 0) {
          return
        }

        try {
          const payload = JSON.parse(trimmed) as unknown
          this.handleJsonLine(payload, {
            emit: (event) => this.emit(event),
            getSessionId: () => this.sessionId,
            setSessionId: (id) => {
              this.sessionId = id
            },
          })
        } catch {
          this.emit({ type: "output.text", text: `${line}\n` })
        }
      }

      child.stdout.on("data", (data: Buffer) => {
        stdoutBuffer += data.toString()
        const lines = stdoutBuffer.split("\n")
        stdoutBuffer = lines.pop() ?? ""
        for (const line of lines) {
          flushStdoutLine(line)
        }
      })

      child.stderr.on("data", (data: Buffer) => {
        const textChunk = data.toString()
        stderrBuffer += textChunk
        this.emit({ type: "output.text", text: textChunk })
      })

      await new Promise<void>((resolve, reject) => {
        child.once("error", reject)
        child.once("close", (code, signal) => {
          if (stdoutBuffer.trim().length > 0) {
            flushStdoutLine(stdoutBuffer)
          }

          this.activeChild = undefined

          if (signal || code === null || code !== 0) {
            const detail = stderrBuffer.trim()
            this.emit({
              type: "session.error",
              message:
                detail.length > 0
                  ? detail
                  : `${this.name} exited with code ${code ?? "unknown"}${signal ? ` (signal: ${signal})` : ""}`,
            })
          }

          resolve()
        })
      })
    }
  }
}
