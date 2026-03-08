import { existsSync } from "node:fs"
import path from "node:path"

import type {
  NormalizedSessionPayload,
  SessionServerEvent,
  SessionStartupInput,
} from "@goddard-ai/session-protocol"
import { AgentSession, SessionManager, createAgentSession } from "@mariozechner/pi-coding-agent"

import { SessionDriver } from "./types.ts"

async function resolveSessionManager(input: SessionStartupInput): Promise<SessionManager> {
  if (!input.resume) {
    return SessionManager.create(process.cwd())
  }

  const candidatePath = path.resolve(input.resume)
  if (existsSync(candidatePath)) {
    return SessionManager.open(candidatePath)
  }

  const sessions = await SessionManager.list(process.cwd())
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

export default class PiDriver extends SessionDriver {
  readonly name = "pi" as const
  private sessionPromise: Promise<void> | undefined
  private session: AgentSession | undefined
  private unsubscribeSession: (() => void) | undefined

  private async getSession() {
    if (this.sessionPromise) {
      await this.sessionPromise
    }
    return this.session
  }

  private onSessionEvent = (event: any) => {
    const session = this.session
    if (!session) {
      return
    }

    if (event.type === "message_update") {
      const update = event.assistantMessageEvent

      if (update?.type === "text_delta" && typeof update.delta === "string") {
        this.emit({ type: "output.text", text: update.delta })
        this.emit({
          type: "output.normalized",
          payload: createPiPayload("delta", event, session.sessionId, {
            role: "assistant",
            text: update.delta,
          }),
        })
        return
      }

      if (update?.type === "toolcall_end") {
        this.emit({
          type: "output.normalized",
          payload: createPiPayload("tool_call", event, session.sessionId, {
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
        this.emit({
          type: "output.normalized",
          payload: createPiPayload("status", event, session.sessionId, {
            done: true,
            message: typeof update.reason === "string" ? update.reason : "done",
          }),
        })
        return
      }

      if (update?.type === "error") {
        this.emit({
          type: "output.normalized",
          payload: createPiPayload("error", event, session.sessionId, {
            message: typeof update.reason === "string" ? update.reason : "message_update:error",
          }),
        })
        return
      }

      this.emit({
        type: "output.normalized",
        payload: createPiPayload("status", event, session.sessionId, {
          message:
            typeof update?.type === "string" ? `message_update:${update.type}` : "message_update",
        }),
      })
      return
    }

    if (event.type === "message_end") {
      this.emit({
        type: "output.normalized",
        payload: createPiPayload("message", event, session.sessionId, {
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
      this.emit({
        type: "output.normalized",
        payload: createPiPayload("tool_call", event, session.sessionId, {
          tool: {
            name: typeof event.toolName === "string" ? event.toolName : undefined,
            arguments: event.args,
          },
        }),
      })
      return
    }

    if (event.type === "tool_execution_end") {
      this.emit({
        type: "output.normalized",
        payload: createPiPayload(
          event.isError ? "error" : "tool_result",
          event,
          session.sessionId,
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

    this.emit({
      type: "output.normalized",
      payload: createPiPayload("status", event, session.sessionId, {
        message: typeof event.type === "string" ? event.type : "event",
      }),
    })
  }

  async start(input: SessionStartupInput) {
    if (this.session || this.sessionPromise) {
      throw new Error("pi session is already running")
    }

    let sessionPromise: Promise<void> | undefined
    sessionPromise = (async () => {
      const sessionManager = await resolveSessionManager(input)
      const { session } = await createAgentSession({
        sessionManager,
      })

      if (sessionPromise !== this.sessionPromise) {
        session.dispose()
        return
      }

      this.session = session
      this.sessionPromise = undefined
      this.unsubscribeSession = session.subscribe(this.onSessionEvent)
    })()

    this.sessionPromise = sessionPromise
    await sessionPromise
  }

  async sendEvent(event: Parameters<SessionDriver["sendEvent"]>[0]) {
    if (event.type !== "input.text") {
      throw new Error("pi only supports input.text events")
    }

    const session = await this.getSession()
    if (session) {
      await session.abort()
      await session.prompt(event.text)
      return
    }

    throw new Error("pi session has not been started")
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
    this.unsubscribeSession?.()
    this.unsubscribeSession = undefined
    this.session?.dispose()
    this.session = undefined
    this.sessionPromise = undefined
  }
}
