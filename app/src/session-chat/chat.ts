import type { DaemonSession, GetDaemonSessionHistoryResponse } from "@goddard-ai/sdk"
import type { SessionTranscriptMessage } from "~/sessions/models.ts"

type SessionHistoryMessage = GetDaemonSessionHistoryResponse["history"][number]

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

function hasMethod(
  value: unknown,
  method: "session/prompt" | "session/update",
): value is Record<string, unknown> & { method: typeof method; params?: unknown } {
  return isRecord(value) && value.method === method
}

function textFromContentBlocks(blocks: unknown): string | null {
  if (!Array.isArray(blocks)) {
    return null
  }

  const text = blocks
    .map((block) =>
      isRecord(block) && block.type === "text" && typeof block.text === "string" ? block.text : "",
    )
    .filter(Boolean)
    .join("\n")
    .trim()

  return text || null
}

function extractPromptText(message: SessionHistoryMessage): string | null {
  if (!hasMethod(message, "session/prompt") || !isRecord(message.params)) {
    return null
  }

  return textFromContentBlocks(message.params.prompt)
}

function collectTextFragments(value: unknown, fragments: string[], depth = 0) {
  if (depth > 5) {
    return
  }

  if (typeof value === "string") {
    const trimmed = value.trim()

    if (trimmed) {
      fragments.push(trimmed)
    }
    return
  }

  const blockText = textFromContentBlocks(value)

  if (blockText) {
    fragments.push(blockText)
    return
  }

  if (Array.isArray(value)) {
    for (const item of value) {
      collectTextFragments(item, fragments, depth + 1)
    }
    return
  }

  if (!isRecord(value)) {
    return
  }

  for (const [key, nestedValue] of Object.entries(value)) {
    if (key === "sessionId" || key === "cwd") {
      continue
    }

    collectTextFragments(nestedValue, fragments, depth + 1)
  }
}

function extractUpdateText(message: SessionHistoryMessage): string | null {
  if (!hasMethod(message, "session/update")) {
    return null
  }

  const fragments: string[] = []
  collectTextFragments(message.params, fragments)
  const uniqueFragments = [...new Set(fragments)]
  return uniqueFragments.join("\n").trim() || null
}

export function buildTranscriptMessages(
  session: DaemonSession,
  history: readonly SessionHistoryMessage[],
) {
  const messages: SessionTranscriptMessage[] = [
    {
      id: `${session.id}:context`,
      role: "system",
      authorName: "System",
      timestampLabel: session.status,
      text: `Working directory: ${session.cwd}`,
    },
  ]

  for (const [index, message] of history.entries()) {
    const promptText = extractPromptText(message)

    if (promptText) {
      messages.push({
        id: `${session.id}:prompt:${index}`,
        role: "user",
        authorName: "You",
        timestampLabel: "Prompt",
        text: promptText,
      })
      continue
    }

    const updateText = extractUpdateText(message)

    if (updateText) {
      messages.push({
        id: `${session.id}:update:${index}`,
        role: "assistant",
        authorName: session.agentName,
        timestampLabel: "Update",
        text: updateText,
      })
    }
  }

  if (
    session.lastAgentMessage &&
    !messages.some(
      (message) => message.role === "assistant" && message.text === session.lastAgentMessage,
    )
  ) {
    messages.push({
      id: `${session.id}:latest`,
      role: "assistant",
      authorName: session.agentName,
      timestampLabel: "Latest",
      text: session.lastAgentMessage,
    })
  }

  return messages
}
