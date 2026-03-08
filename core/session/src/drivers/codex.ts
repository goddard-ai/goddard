import type { NormalizedSessionPayload } from "@goddard-ai/session-protocol"

import { JsonLineSubprocess } from "./subprocess.ts"
import type { SessionDriverInput } from "./types.ts"

export function buildCodexArgs(input: SessionDriverInput): string[] {
  const args = ["exec"]

  if (input.resume) {
    args.push("resume", input.resume)
  }

  args.push("--json")

  if (input.initialPrompt) {
    args.push(input.initialPrompt)
  }

  return args
}

function createCodexPayload(
  kind: NormalizedSessionPayload["kind"],
  raw: unknown,
  extra: Partial<NormalizedSessionPayload> = {},
): NormalizedSessionPayload {
  return {
    schemaVersion: 1,
    source: {
      driver: "codex",
      format: "json-line",
    },
    kind,
    raw,
    ...extra,
  } as NormalizedSessionPayload
}

export default class CodexDriver extends JsonLineSubprocess.SessionDriver {
  constructor() {
    super({
      name: "codex",
      command: "codex",
    })
  }

  protected buildArgs(text: string, threadId?: string) {
    return buildCodexArgs({
      resume: threadId,
      initialPrompt: text,
    })
  }

  protected handleJsonLine(payload: unknown, context: JsonLineSubprocess.HandlerContext) {
    const event = payload as Record<string, unknown>
    const type = typeof event.type === "string" ? event.type : ""

    if (type === "thread.started") {
      const threadId = typeof event.thread_id === "string" ? event.thread_id : undefined
      if (threadId) {
        context.setSessionId(threadId)
      }
      context.emit({
        type: "output.normalized",
        payload: createCodexPayload("status", payload, {
          id: threadId,
          message: type,
        }),
      })
      return
    }

    if (type === "turn.started") {
      context.emit({
        type: "output.normalized",
        payload: createCodexPayload("status", payload, {
          id: context.getSessionId(),
          message: type,
        }),
      })
      return
    }

    if (type === "item.completed") {
      const item = event.item as Record<string, unknown> | undefined
      if (item?.type === "agent_message" && typeof item.text === "string") {
        context.emit({ type: "output.text", text: item.text })
        context.emit({
          type: "output.normalized",
          payload: createCodexPayload("message", payload, {
            id: typeof item.id === "string" ? item.id : context.getSessionId(),
            role: "assistant",
            text: item.text,
          }),
        })
        return
      }
    }

    if (type === "turn.completed") {
      const usage = event.usage as Record<string, unknown> | undefined
      const inputTokens = typeof usage?.input_tokens === "number" ? usage.input_tokens : undefined
      const outputTokens = typeof usage?.output_tokens === "number" ? usage.output_tokens : undefined
      context.emit({
        type: "output.normalized",
        payload: createCodexPayload("usage", payload, {
          id: context.getSessionId(),
          done: true,
          usage: {
            inputTokens,
            outputTokens,
            totalTokens:
              inputTokens !== undefined && outputTokens !== undefined
                ? inputTokens + outputTokens
                : undefined,
          },
        }),
      })
      return
    }

    context.emit({
      type: "output.normalized",
      payload: createCodexPayload("unknown", payload, {
        id: context.getSessionId(),
      }),
    })
  }
}
