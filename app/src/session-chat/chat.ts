import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import type {
  SessionTranscriptItem,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
  SessionTranscriptToolContent,
  SessionTranscriptToolKind,
  SessionTranscriptToolLocation,
  SessionTranscriptToolStatus,
} from "~/sessions/models.ts"

type SessionHistoryMessage = GetSessionHistoryResponse["history"][number]

type ParsedToolCallUpdate = {
  updateKind: "tool_call" | "tool_call_update"
  toolCallId: string
  title?: string
  toolKind?: SessionTranscriptToolKind
  status?: SessionTranscriptToolStatus
  content?: SessionTranscriptToolContent[]
  locations?: SessionTranscriptToolLocation[]
}

const TOOL_KINDS = new Set<SessionTranscriptToolKind>([
  "read",
  "edit",
  "delete",
  "move",
  "search",
  "execute",
  "think",
  "fetch",
  "switch_mode",
  "other",
])

const TOOL_STATUSES = new Set<SessionTranscriptToolStatus>([
  "pending",
  "in_progress",
  "completed",
  "failed",
])

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

function hasMethod(
  value: unknown,
  method: "session/prompt" | "session/update",
): value is Record<string, unknown> & { method: typeof method; params?: unknown } {
  return isRecord(value) && value.method === method
}

function textFromContentBlocks(blocks: unknown) {
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

function extractPromptText(message: SessionHistoryMessage) {
  if (!hasMethod(message, "session/prompt") || !isRecord(message.params)) {
    return null
  }

  return textFromContentBlocks(message.params.prompt)
}

function extractPromptRequestId(message: SessionHistoryMessage) {
  if (!hasMethod(message, "session/prompt") || !("id" in message) || message.id == null) {
    return null
  }

  return String(message.id)
}

function extractSessionUpdate(message: SessionHistoryMessage) {
  if (!hasMethod(message, "session/update") || !isRecord(message.params)) {
    return null
  }

  return isRecord(message.params.update) ? message.params.update : null
}

function extractToolKind(value: unknown) {
  return typeof value === "string" && TOOL_KINDS.has(value as SessionTranscriptToolKind)
    ? (value as SessionTranscriptToolKind)
    : undefined
}

function extractToolStatus(value: unknown) {
  return typeof value === "string" && TOOL_STATUSES.has(value as SessionTranscriptToolStatus)
    ? (value as SessionTranscriptToolStatus)
    : undefined
}

function extractToolCallLocations(value: unknown): SessionTranscriptToolLocation[] {
  if (!Array.isArray(value)) {
    return []
  }

  const locations: SessionTranscriptToolLocation[] = []

  for (const location of value) {
    if (!isRecord(location) || typeof location.path !== "string") {
      continue
    }

    locations.push({
      path: location.path,
      line: typeof location.line === "number" ? location.line : null,
    })
  }

  return locations
}

function extractToolCallContent(value: unknown): SessionTranscriptToolContent[] {
  if (!Array.isArray(value)) {
    return []
  }

  const content: SessionTranscriptToolContent[] = []

  for (const item of value) {
    if (!isRecord(item) || typeof item.type !== "string") {
      continue
    }

    if (item.type === "content") {
      content.push({
        type: "content",
        text: textFromContentBlocks(item.content),
      })
      continue
    }

    if (item.type === "diff") {
      content.push({
        type: "diff",
        path: typeof item.path === "string" ? item.path : null,
        oldText: typeof item.oldText === "string" ? item.oldText : null,
        newText: typeof item.newText === "string" ? item.newText : null,
      })
      continue
    }

    if (item.type === "terminal" && typeof item.terminalId === "string") {
      content.push({
        type: "terminal",
        terminalId: item.terminalId,
      })
    }
  }

  return content
}

/** Extracts one structured tool-call update so the transcript can preserve ACP row identity. */
function extractToolCallUpdate(message: SessionHistoryMessage) {
  const update = extractSessionUpdate(message)

  if (
    !update ||
    (update.sessionUpdate !== "tool_call" && update.sessionUpdate !== "tool_call_update") ||
    typeof update.toolCallId !== "string"
  ) {
    return null
  }

  const toolCallUpdate: ParsedToolCallUpdate = {
    updateKind: update.sessionUpdate,
    toolCallId: update.toolCallId,
  }

  if (typeof update.title === "string" && update.title.trim().length > 0) {
    toolCallUpdate.title = update.title.trim()
  }

  const toolKind = extractToolKind(update.kind)
  if (toolKind) {
    toolCallUpdate.toolKind = toolKind
  }

  const status = extractToolStatus(update.status)
  if (status) {
    toolCallUpdate.status = status
  }

  if ("content" in update) {
    toolCallUpdate.content = extractToolCallContent(update.content)
  }

  if ("locations" in update) {
    toolCallUpdate.locations = extractToolCallLocations(update.locations)
  }

  return toolCallUpdate
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

function extractUpdateText(message: SessionHistoryMessage) {
  if (!hasMethod(message, "session/update")) {
    return null
  }

  if (extractToolCallUpdate(message)) {
    return null
  }

  const fragments: string[] = []
  collectTextFragments(message.params, fragments)
  const uniqueFragments = [...new Set(fragments)]
  return uniqueFragments.join("\n").trim() || null
}

function isPromptCompletionMessage(message: SessionHistoryMessage) {
  if (!isRecord(message) || !("id" in message) || message.id == null) {
    return false
  }

  return (
    ("result" in message && message.result != null) || ("error" in message && message.error != null)
  )
}

function fallbackToolTitle(toolKind: SessionTranscriptToolKind) {
  if (toolKind === "switch_mode") {
    return "Switch mode"
  }

  return `${toolKind.slice(0, 1).toUpperCase()}${toolKind.slice(1)} tool`
}

/** Applies one ACP tool update to the transcript, updating an existing card when identities match. */
function applyToolCallUpdate(props: {
  items: SessionTranscriptItem[]
  toolRowIndexes: Map<string, number>
  session: DaemonSession
  turnId: string
  toolCallUpdate: ParsedToolCallUpdate
}) {
  const rowKey = `${props.turnId}:tool:${props.toolCallUpdate.toolCallId}`
  const rowIndex = props.toolRowIndexes.get(rowKey)

  if (rowIndex == null) {
    const toolKind = props.toolCallUpdate.toolKind ?? "other"
    const toolRow: SessionTranscriptToolCall = {
      kind: "toolCall",
      id: rowKey,
      toolCallId: props.toolCallUpdate.toolCallId,
      authorName: props.session.agentName,
      timestampLabel: "Tool",
      title: props.toolCallUpdate.title ?? fallbackToolTitle(toolKind),
      toolKind,
      status:
        props.toolCallUpdate.status ??
        (props.toolCallUpdate.updateKind === "tool_call" ? "in_progress" : "pending"),
      content: props.toolCallUpdate.content ?? [],
      locations: props.toolCallUpdate.locations ?? [],
    }

    props.toolRowIndexes.set(rowKey, props.items.push(toolRow) - 1)
    return
  }

  const existingRow = props.items[rowIndex]
  if (existingRow.kind !== "toolCall") {
    return
  }

  props.items[rowIndex] = {
    ...existingRow,
    title: props.toolCallUpdate.title ?? existingRow.title,
    toolKind: props.toolCallUpdate.toolKind ?? existingRow.toolKind,
    status: props.toolCallUpdate.status ?? existingRow.status,
    content: props.toolCallUpdate.content ?? existingRow.content,
    locations: props.toolCallUpdate.locations ?? existingRow.locations,
  }
}

function closePromptTurn(message: SessionHistoryMessage, activePromptIds: string[]) {
  if (!isPromptCompletionMessage(message) || !("id" in message) || message.id == null) {
    return
  }

  const resolvedPromptId = String(message.id)
  const promptIndex = activePromptIds.lastIndexOf(resolvedPromptId)

  if (promptIndex > -1) {
    activePromptIds.splice(promptIndex, 1)
  }
}

function createTextRow(
  input: Omit<SessionTranscriptTextMessage, "kind">,
): SessionTranscriptTextMessage {
  return {
    kind: "message",
    ...input,
  }
}

export function buildTranscriptMessages(
  session: DaemonSession,
  history: readonly SessionHistoryMessage[],
) {
  const items: SessionTranscriptItem[] = [
    createTextRow({
      id: `${session.id}:context`,
      role: "system",
      authorName: "System",
      timestampLabel: session.status,
      text: `Working directory: ${session.cwd}`,
    }),
  ]
  const activePromptIds: string[] = []
  const toolRowIndexes = new Map<string, number>()

  for (const [index, message] of history.entries()) {
    const promptText = extractPromptText(message)

    if (promptText) {
      const promptRequestId = extractPromptRequestId(message)
      if (promptRequestId) {
        activePromptIds.push(promptRequestId)
      }

      items.push(
        createTextRow({
          id: `${session.id}:prompt:${index}`,
          role: "user",
          authorName: "You",
          timestampLabel: "Prompt",
          text: promptText,
        }),
      )
      closePromptTurn(message, activePromptIds)
      continue
    }

    const toolCallUpdate = extractToolCallUpdate(message)

    if (toolCallUpdate) {
      applyToolCallUpdate({
        items,
        toolRowIndexes,
        session,
        turnId: activePromptIds.at(-1) ?? `${session.id}:orphan`,
        toolCallUpdate,
      })
      closePromptTurn(message, activePromptIds)
      continue
    }

    const updateText = extractUpdateText(message)

    if (updateText) {
      items.push(
        createTextRow({
          id: `${session.id}:update:${index}`,
          role: "assistant",
          authorName: session.agentName,
          timestampLabel: "Update",
          text: updateText,
        }),
      )
    }

    closePromptTurn(message, activePromptIds)
  }

  if (
    session.lastAgentMessage &&
    !items.some(
      (item) =>
        item.kind === "message" &&
        item.role === "assistant" &&
        item.text === session.lastAgentMessage,
    )
  ) {
    items.push(
      createTextRow({
        id: `${session.id}:latest`,
        role: "assistant",
        authorName: session.agentName,
        timestampLabel: "Latest",
        text: session.lastAgentMessage,
        streaming: session.activeDaemonSession && session.status === "active",
      }),
    )
  }

  return items
}
