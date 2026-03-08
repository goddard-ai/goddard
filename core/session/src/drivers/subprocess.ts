import { ChildProcessByStdio, spawn, type ChildProcessWithoutNullStreams } from "node:child_process"

import {
  sessionServerEventSchema,
  type SessionClientEvent,
  type SessionServerEvent,
  type SessionStartupInput,
} from "@goddard-ai/session-protocol"

import type { SessionDriverName } from "./types.ts"
import { Readable } from "node:stream"

export declare namespace JsonLineSubprocess {
  interface ServerDriverOptions {
    name: SessionDriverName
    command: string
    cwd?: string
    buildArgs: (text: string, sessionId?: string) => string[]
    handleJsonLine: (
      payload: unknown,
      context: {
        emit: (event: SessionServerEvent) => void
        getSessionId: () => string | undefined
        setSessionId: (id: string) => void
      },
    ) => void
  }
}

type ChildProcess = ChildProcessByStdio<null, Readable, Readable>

export const JsonLineSubprocess = {
  createSessionDriver(options: JsonLineSubprocess.ServerDriverOptions) {
    const listeners = new Set<(event: SessionServerEvent) => void>()
    let sessionId: string | undefined
    let activeChild: ChildProcess | undefined
    let closed = false
    let queue = Promise.resolve()

    const emit = (event: SessionServerEvent) => {
      event = sessionServerEventSchema.parse(event)
      for (const listener of listeners) {
        listener(event)
      }
    }

    const runTurn = async (text: string) => {
      if (closed) {
        throw new Error(`${options.name} driver is closed`)
      }

      const child = spawn(options.command, options.buildArgs(text, sessionId), {
        stdio: ["ignore", "pipe", "pipe"],
        cwd: options.cwd,
        env: process.env,
      })
      activeChild = child

      let stdoutBuffer = ""
      let stderrBuffer = ""

      const flushStdoutLine = (line: string) => {
        const trimmed = line.trim()
        if (trimmed.length === 0) {
          return
        }

        try {
          const payload = JSON.parse(trimmed) as unknown
          options.handleJsonLine(payload, {
            emit,
            getSessionId: () => sessionId,
            setSessionId: (id: string) => {
              sessionId = id
            },
          })
        } catch {
          emit({ type: "output.text", text: `${line}\n` })
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
        emit({ type: "output.text", text: textChunk })
      })

      await new Promise<void>((resolve, reject) => {
        child.once("error", reject)
        child.once("close", (code, signal) => {
          if (stdoutBuffer.trim().length > 0) {
            flushStdoutLine(stdoutBuffer)
          }

          activeChild = undefined

          if (signal || code === null || code !== 0) {
            const detail = stderrBuffer.trim()
            emit({
              type: "session.error",
              message:
                detail.length > 0
                  ? detail
                  : `${options.name} exited with code ${code ?? "unknown"}${signal ? ` (signal: ${signal})` : ""}`,
            })
          }

          resolve()
        })
      })
    }

    return {
      name: options.name,
      start: (input: SessionStartupInput) => {
        sessionId = input.resume
      },
      sendEvent: async (event: SessionClientEvent) => {
        if (event.type !== "input.text") {
          throw new Error(`${options.name} only supports input.text events`)
        }

        queue = queue.then(
          async () => await runTurn(event.text),
          async () => await runTurn(event.text),
        )
        await queue
      },
      onEvent: (listener: (event: SessionServerEvent) => void) => {
        listeners.add(listener)
        return () => {
          listeners.delete(listener)
        }
      },
      getCapabilities: () => ({
        terminal: {
          enabled: false,
          canResize: false,
          hasScreenState: false,
        },
        normalizedOutput: true,
      }),
      close: () => {
        closed = true
        activeChild?.kill()
        activeChild = undefined
      },
    }
  },
}
