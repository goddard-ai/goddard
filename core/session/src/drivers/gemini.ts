import type { NormalizedSessionPayload } from "@goddard-ai/session-protocol"

import { JsonLineSubprocess } from "./subprocess.ts"
import type { SessionDriverInput } from "./types.ts"

export function buildGeminiArgs(input: SessionDriverInput): string[] {
  const args = ["--output-format", "stream-json"]

  if (input.resume) {
    args.push("--resume", input.resume)
  }

  if (input.initialPrompt) {
    args.push("--prompt", input.initialPrompt)
  }

  return args
}

function createGeminiPayload(
  kind: NormalizedSessionPayload["kind"],
  raw: unknown,
  extra: Partial<NormalizedSessionPayload> = {},
): NormalizedSessionPayload {
  return {
    schemaVersion: 1,
    source: {
      driver: "gemini",
      format: "json-line",
    },
    kind,
    raw,
    ...extra,
  } as NormalizedSessionPayload
}

export default class GeminiDriver extends JsonLineSubprocess.SessionDriver {
  constructor() {
    super({
      name: "gemini",
      command: "gemini",
    })
  }

  protected buildArgs(text: string, sessionId?: string) {
    return buildGeminiArgs({
      resume: sessionId,
      initialPrompt: text,
    })
  }

  protected handleJsonLine(payload: unknown, context: JsonLineSubprocess.HandlerContext) {
    const event = payload as Record<string, unknown>
    const type = typeof event.type === "string" ? event.type : ""

    if (type === "init") {
      const sessionId = typeof event.session_id === "string" ? event.session_id : undefined
      if (sessionId) {
        context.setSessionId(sessionId)
      }
      context.emit({
        type: "output.normalized",
        payload: createGeminiPayload("status", payload, {
          id: sessionId,
          message: typeof event.model === "string" ? `init:${event.model}` : "init",
        }),
      })
      return
    }

    if (type === "message") {
      const role =
        event.role === "assistant" || event.role === "user" || event.role === "system"
          ? event.role
          : undefined
      const text = typeof event.content === "string" ? event.content : undefined
      const isDelta = event.delta === true

      if (role === "assistant" && text) {
        context.emit({ type: "output.text", text })
      }

      context.emit({
        type: "output.normalized",
        payload: createGeminiPayload(isDelta ? "delta" : "message", payload, {
          id: context.getSessionId(),
          role,
          text,
        }),
      })
      return
    }

    if (type === "result") {
      const stats = event.stats as Record<string, unknown> | undefined
      const inputTokens = typeof stats?.input_tokens === "number" ? stats.input_tokens : undefined
      const outputTokens = typeof stats?.output_tokens === "number" ? stats.output_tokens : undefined
      const totalTokens = typeof stats?.total_tokens === "number" ? stats.total_tokens : undefined

      context.emit({
        type: "output.normalized",
        payload: createGeminiPayload("usage", payload, {
          id: context.getSessionId(),
          done: true,
          usage: {
            inputTokens,
            outputTokens,
            totalTokens,
          },
        }),
      })

      context.emit({
        type: "output.normalized",
        payload: createGeminiPayload(event.status === "success" ? "status" : "error", payload, {
          id: context.getSessionId(),
          done: true,
          message: typeof event.status === "string" ? event.status : undefined,
        }),
      })
      return
    }

    context.emit({
      type: "output.normalized",
      payload: createGeminiPayload("unknown", payload, {
        id: context.getSessionId(),
      }),
    })
  }
}
