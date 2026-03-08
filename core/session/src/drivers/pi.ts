import { existsSync } from "node:fs"
import path from "node:path"

import type {
  NormalizedSessionPayload,
  SessionServerEvent,
  SessionStartupInput,
} from "@goddard-ai/session-protocol"
import { SessionManager, createAgentSession } from "@mariozechner/pi-coding-agent"

import type { SessionDriver } from "./types.ts"

async function resolveSessionManager(
  input: SessionStartupInput,
  cwd: string,
): Promise<SessionManager> {
  if (!input.resume) {
    return SessionManager.create(cwd)
  }

  const candidatePath = path.resolve(cwd, input.resume)
  if (existsSync(candidatePath)) {
    return SessionManager.open(candidatePath)
  }

  const sessions = await SessionManager.list(cwd)
  const matches = sessions.filter(
    (session) => session.id === input.resume || session.id.startsWith(input.resume || ""),
  )

  if (matches.length === 1) {
    return SessionManager.open(matches[0].path)
  }

  if (matches.length > 1) {
    throw new Error(`Resume target "${input.resume}" is ambiguous. Provide a full session id.`)
  }

  throw new Error(`Unable to resolve pi session "${input.resume}"`)
}

function getPiMessageText(message: unknown): string | undefined {
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

function createPiPayload(
  kind: NormalizedSessionPayload["kind"],
  raw: unknown,
  sessionId: string,
  extra: Partial<NormalizedSessionPayload> = {},
): NormalizedSessionPayload {
  return {
    schemaVersion: 1,
    source: {
      driver: "pi",
      format: "json-line",
    },
    kind,
    id: sessionId,
    raw,
    ...extra,
  } as NormalizedSessionPayload
}

const listeners = new Set<(event: SessionServerEvent) => void>()
let serverSessionPromise:
  | Promise<Awaited<ReturnType<typeof createAgentSession>>["session"]>
  | undefined
let serverSession: Awaited<ReturnType<typeof createAgentSession>>["session"] | undefined
let serverUnsubscribe: (() => void) | undefined
let serverQueue = Promise.resolve()
let serverStartupInput: SessionStartupInput = {}

function emit(event: SessionServerEvent) {
  for (const listener of listeners) {
    listener(event)
  }
}

async function ensureServerSession() {
  if (serverSession) {
    return serverSession
  }

  if (!serverSessionPromise) {
    serverSessionPromise = (async () => {
      const sessionManager = await resolveSessionManager(serverStartupInput, process.cwd())
      const created = await createAgentSession({
        cwd: process.cwd(),
        sessionManager,
      })
      serverSession = created.session
      serverUnsubscribe = created.session.subscribe((event: any) => {
        if (event.type === "message_update") {
          const update = event.assistantMessageEvent

          if (update?.type === "text_delta" && typeof update.delta === "string") {
            emit({ type: "output.text", text: update.delta })
            emit({
              type: "output.normalized",
              payload: createPiPayload("delta", event, created.session.sessionId, {
                role: "assistant",
                text: update.delta,
              }),
            })
            return
          }

          if (update?.type === "toolcall_end") {
            emit({
              type: "output.normalized",
              payload: createPiPayload("tool_call", event, created.session.sessionId, {
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
              payload: createPiPayload("status", event, created.session.sessionId, {
                done: true,
                message: typeof update.reason === "string" ? update.reason : "done",
              }),
            })
            return
          }

          if (update?.type === "error") {
            emit({
              type: "output.normalized",
              payload: createPiPayload("error", event, created.session.sessionId, {
                message: typeof update.reason === "string" ? update.reason : "message_update:error",
              }),
            })
            return
          }

          emit({
            type: "output.normalized",
            payload: createPiPayload("status", event, created.session.sessionId, {
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
            payload: createPiPayload("message", event, created.session.sessionId, {
              role:
                event.message?.role === "assistant" ||
                event.message?.role === "user" ||
                event.message?.role === "system" ||
                event.message?.role === "tool"
                  ? event.message.role
                  : undefined,
              text: getPiMessageText(event.message),
            }),
          })
          return
        }

        if (event.type === "tool_execution_start") {
          emit({
            type: "output.normalized",
            payload: createPiPayload("tool_call", event, created.session.sessionId, {
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
            payload: createPiPayload(
              event.isError ? "error" : "tool_result",
              event,
              created.session.sessionId,
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
          payload: createPiPayload("status", event, created.session.sessionId, {
            message: typeof event.type === "string" ? event.type : "event",
          }),
        })
      })

      return created.session
    })()
  }

  return await serverSessionPromise
}

export const driver: SessionDriver = {
  name: "pi",
  start: async (input) => {
    if (serverSession || serverSessionPromise) {
      if (input.resume !== serverStartupInput.resume) {
        throw new Error("pi session has already been started")
      }
      return
    }

    serverStartupInput = { ...input }
  },
  sendEvent: async (event) => {
    if (event.type !== "input.text") {
      throw new Error("pi only supports input.text events")
    }

    serverQueue = serverQueue.then(
      async () => {
        const session = await ensureServerSession()
        await session.prompt(event.text)
      },
      async () => {
        const session = await ensureServerSession()
        await session.prompt(event.text)
      },
    )

    await serverQueue
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
    serverUnsubscribe?.()
    serverUnsubscribe = undefined
    serverSession?.dispose()
    serverSession = undefined
    serverSessionPromise = undefined
    serverQueue = Promise.resolve()
    serverStartupInput = {}
  },
}
