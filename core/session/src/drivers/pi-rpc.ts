import { spawn } from "node:child_process"
import { createRequire } from "node:module"

import type {
  NormalizedSessionPayload,
  SessionServerEvent,
  SessionStartupInput,
} from "@goddard-ai/session-protocol"

import type { SessionDriver } from "./types.ts"

const require = createRequire(import.meta.url)
const rpcClientPath = require.resolve("@mariozechner/pi-coding-agent/dist/modes/rpc/rpc-client.js")
const { RpcClient } = await import(rpcClientPath)

function createPiRpcPayload(
  kind: NormalizedSessionPayload["kind"],
  raw: unknown,
  extra: Partial<NormalizedSessionPayload> = {},
): NormalizedSessionPayload {
  return {
    schemaVersion: 1,
    source: {
      driver: "pi-rpc",
      format: "json-line",
    },
    kind,
    raw,
    ...extra,
  } as NormalizedSessionPayload
}

class ExternalRpcClient extends (RpcClient as any) {
  async start() {
    const args = ["--mode", "rpc", ...(this.options.args || [])]
    this.process = spawn("pi", args, {
      cwd: this.options.cwd,
      env: { ...process.env, ...this.options.env },
      stdio: ["pipe", "pipe", "pipe"],
    })

    let stdoutBuffer = ""
    this.process.stdout.on("data", (data: Buffer) => {
      stdoutBuffer += data.toString()
      const lines = stdoutBuffer.split("\n")
      stdoutBuffer = lines.pop() ?? ""
      for (const line of lines) {
        if (line.trim()) {
          this.handleLine(line)
        }
      }
    })

    this.process.stderr.on("data", (data: Buffer) => {
      this.stderr += data.toString()
    })

    return await new Promise<void>((resolve, reject) => {
      const onStart = (event: any) => {
        if (event.type === "agent_start") {
          this.eventListeners.delete(onStart)
          resolve()
        }
      }
      this.onEvent(onStart)

      this.process.on("error", reject)
      this.process.on("exit", (code: number) => {
        if (code !== 0 && code !== null) {
          reject(new Error(`Agent process exited with code ${code}. Stderr: ${this.stderr}`))
        }
      })
    })
  }

  async stop() {
    if (this.process) {
      this.process.kill()
    }
  }
}

function getRpcMessageText(message: unknown): string | undefined {
  if (!message || typeof message !== "object") {
    return undefined
  }

  const content = (message as { content?: unknown }).content
  if (!Array.isArray(content)) {
    return undefined
  }

  return content
    .map((part) =>
      part && typeof part === "object" && (part as { type?: unknown }).type === "text"
        ? (part as { text?: unknown }).text
        : undefined,
    )
    .filter((value): value is string => typeof value === "string")
    .join("")
}

const listeners = new Set<(event: SessionServerEvent) => void>()
let rpcClient: ExternalRpcClient | undefined
let rpcClientPromise: Promise<ExternalRpcClient> | undefined
let rpcQueue = Promise.resolve()
let rpcStartupInput: SessionStartupInput = {}

function emit(event: SessionServerEvent) {
  for (const listener of listeners) {
    listener(event)
  }
}

async function ensureRpcClient() {
  if (rpcClient) {
    return rpcClient
  }

  if (!rpcClientPromise) {
    rpcClientPromise = (async () => {
      const client = new ExternalRpcClient({
        cwd: process.cwd(),
        args: rpcStartupInput.resume ? ["--session", rpcStartupInput.resume] : ["--no-session"],
      })

      client.onEvent((event: any) => {
        if (event.type === "message_update") {
          const update = event.assistantMessageEvent

          if (update?.type === "text_delta" && typeof update.delta === "string") {
            emit({ type: "output.text", text: update.delta })
            emit({
              type: "output.normalized",
              payload: createPiRpcPayload("delta", event, {
                role: "assistant",
                text: update.delta,
              }),
            })
            return
          }

          if (update?.type === "toolcall_end") {
            emit({
              type: "output.normalized",
              payload: createPiRpcPayload("tool_call", event, {
                tool: {
                  name:
                    typeof update.toolCall?.toolName === "string"
                      ? update.toolCall.toolName
                      : undefined,
                  arguments: update.toolCall,
                },
              }),
            })
            return
          }

          if (update?.type === "done") {
            emit({
              type: "output.normalized",
              payload: createPiRpcPayload("status", event, {
                done: true,
                message: typeof update.reason === "string" ? update.reason : "done",
              }),
            })
            return
          }

          if (update?.type === "error") {
            emit({
              type: "output.normalized",
              payload: createPiRpcPayload("error", event, {
                message: typeof update.reason === "string" ? update.reason : "message_update:error",
              }),
            })
            return
          }

          emit({
            type: "output.normalized",
            payload: createPiRpcPayload("status", event, {
              message:
                typeof update?.type === "string"
                  ? `message_update:${update.type}`
                  : "message_update",
            }),
          })
          return
        }

        if (event.type === "message_end") {
          emit({
            type: "output.normalized",
            payload: createPiRpcPayload("message", event, {
              role:
                event.message?.role === "assistant" ||
                event.message?.role === "user" ||
                event.message?.role === "system" ||
                event.message?.role === "tool"
                  ? event.message.role
                  : undefined,
              text: getRpcMessageText(event.message),
            }),
          })
          return
        }

        if (event.type === "tool_execution_start") {
          emit({
            type: "output.normalized",
            payload: createPiRpcPayload("tool_call", event, {
              tool: {
                name: typeof event.toolName === "string" ? event.toolName : undefined,
                arguments: event.args,
              },
            }),
          })
          return
        }

        if (event.type === "tool_execution_end") {
          emit({
            type: "output.normalized",
            payload: createPiRpcPayload(
              event.isError ? "error" : "tool_result",
              event,
              event.isError
                ? {
                    message:
                      typeof event.toolName === "string"
                        ? `${event.toolName} failed`
                        : "tool execution failed",
                  }
                : {
                    tool: {
                      name: typeof event.toolName === "string" ? event.toolName : undefined,
                      result: event.result,
                    },
                  },
            ),
          })
          return
        }

        emit({
          type: "output.normalized",
          payload: createPiRpcPayload("status", event, {
            message: typeof event.type === "string" ? event.type : "event",
          }),
        })
      })

      await client.start()
      rpcClient = client
      return client
    })()
  }

  return await rpcClientPromise
}

export const driver: SessionDriver = {
  name: "pi-rpc",
  start: async (input) => {
    if (rpcClient || rpcClientPromise) {
      if (input.resume !== rpcStartupInput.resume) {
        throw new Error("pi-rpc session has already been started")
      }
      return
    }

    rpcStartupInput = { ...input }
  },
  sendEvent: async (event) => {
    if (event.type !== "input.text") {
      throw new Error("pi-rpc only supports input.text events")
    }

    rpcQueue = rpcQueue.then(
      async () => {
        const client = await ensureRpcClient()
        await client.prompt(event.text)
        await client.waitForIdle()
      },
      async () => {
        const client = await ensureRpcClient()
        await client.prompt(event.text)
        await client.waitForIdle()
      },
    )

    await rpcQueue
  },
  onEvent: (listener) => {
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
    void rpcClient?.stop()
    rpcClient = undefined
    rpcClientPromise = undefined
    rpcQueue = Promise.resolve()
    rpcStartupInput = {}
  },
}
