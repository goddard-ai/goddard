import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"

import type {
  SessionTranscriptContentBlock,
  SessionTranscriptItem,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
  SessionTranscriptToolContent,
  SessionTranscriptToolKind,
  SessionTranscriptToolLocation,
  SessionTranscriptToolStatus,
} from "~/sessions/models.ts"
import { promptBlocksToTranscriptContent } from "./composer-content.ts"

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

type ParsedTranscriptSessionUpdate =
  | {
      kind: "agentMessageChunk"
      text: string
    }
  | {
      kind: "toolCall"
      toolCallUpdate: ParsedToolCallUpdate
    }
  | {
      kind: "ignored"
    }
  | {
      kind: "unsupported"
      reason: string
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

const TRANSCRIPT_IGNORED_SESSION_UPDATES = new Set([
  "available_commands_update",
  "config_option_update",
  "current_mode_update",
  "session_info_update",
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
  const content = promptBlocksToTranscriptContent(blocks)
  const text = content
    .flatMap((block) => (block.type === "text" ? [block.text] : []))
    .join("\n")
    .trim()

  return text || null
}

function textContentBlock(text: string): SessionTranscriptContentBlock {
  return {
    type: "text",
    text,
  }
}

function extractPromptContent(message: SessionHistoryMessage) {
  if (!hasMethod(message, "session/prompt") || !isRecord(message.params)) {
    return []
  }

  return promptBlocksToTranscriptContent(message.params.prompt)
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
function extractToolCallUpdate(update: Record<string, unknown> | null) {
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

function extractAgentMessageChunkText(update: Record<string, unknown>) {
  if (update.sessionUpdate !== "agent_message_chunk") {
    return undefined
  }

  if (
    !isRecord(update.content) ||
    update.content.type !== "text" ||
    typeof update.content.text !== "string"
  ) {
    return null
  }

  return update.content.text
}

function parseTranscriptSessionUpdate(
  message: SessionHistoryMessage,
): ParsedTranscriptSessionUpdate | null {
  if (!hasMethod(message, "session/update")) {
    return null
  }

  const update = extractSessionUpdate(message)

  if (!update || typeof update.sessionUpdate !== "string") {
    return {
      kind: "unsupported",
      reason: "session/update is missing a string sessionUpdate discriminator.",
    }
  }

  const toolCallUpdate = extractToolCallUpdate(update)

  if (toolCallUpdate) {
    return {
      kind: "toolCall",
      toolCallUpdate,
    }
  }

  const agentMessageChunkText = extractAgentMessageChunkText(update)

  if (agentMessageChunkText !== undefined) {
    if (agentMessageChunkText === null) {
      return {
        kind: "unsupported",
        reason: "agent_message_chunk is missing text content.",
      }
    }

    return {
      kind: "agentMessageChunk",
      text: agentMessageChunkText,
    }
  }

  if (TRANSCRIPT_IGNORED_SESSION_UPDATES.has(update.sessionUpdate)) {
    return {
      kind: "ignored",
    }
  }

  return {
    kind: "unsupported",
    reason: `Unsupported transcript session/update payload: ${update.sessionUpdate}`,
  }
}

function reportUnsupportedTranscriptMessage(message: SessionHistoryMessage, reason: string) {
  console.error("Unsupported session-chat transcript message.", {
    reason,
    message,
  })
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

function applyAgentMessageChunk(props: {
  agentRowIndexes: Map<string, number>
  items: SessionTranscriptItem[]
  session: DaemonSession
  text: string
  turnId: string
}) {
  if (props.text.length === 0) {
    return
  }

  const rowKey = `${props.turnId}:agent`
  const rowIndex = props.agentRowIndexes.get(rowKey)

  if (rowIndex == null) {
    props.agentRowIndexes.set(
      rowKey,
      props.items.push(
        createTextRow({
          id: rowKey,
          role: "assistant",
          authorName: props.session.agentName,
          timestampLabel: "Update",
          content: [textContentBlock(props.text)],
          streaming: true,
        }),
      ) - 1,
    )
    return
  }

  const existingRow = props.items[rowIndex]

  if (existingRow?.kind !== "message" || existingRow.role !== "assistant") {
    console.error("Session-chat transcript agent row is in an invalid state.", {
      existingRow,
      rowKey,
    })
    return
  }

  const existingText = existingRow.content
    .flatMap((block) => (block.type === "text" ? [block.text] : []))
    .join("")

  props.items[rowIndex] = {
    ...existingRow,
    content: [textContentBlock(`${existingText}${props.text}`)],
    streaming: true,
  }
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

function closePromptTurn(props: {
  activePromptIds: string[]
  agentRowIndexes: Map<string, number>
  items: SessionTranscriptItem[]
  message: SessionHistoryMessage
}) {
  if (
    !isPromptCompletionMessage(props.message) ||
    !("id" in props.message) ||
    props.message.id == null
  ) {
    return
  }

  const resolvedPromptId = String(props.message.id)
  const promptIndex = props.activePromptIds.lastIndexOf(resolvedPromptId)

  const agentRowIndex = props.agentRowIndexes.get(`${resolvedPromptId}:agent`)

  if (agentRowIndex != null) {
    const agentRow = props.items[agentRowIndex]

    if (agentRow?.kind === "message" && agentRow.role === "assistant" && agentRow.streaming) {
      props.items[agentRowIndex] = {
        ...agentRow,
        streaming: false,
      }
    }
  }

  if (promptIndex > -1) {
    props.activePromptIds.splice(promptIndex, 1)
  }
}

function createTextRow(input: Omit<SessionTranscriptTextMessage, "kind">) {
  return {
    kind: "message",
    ...input,
  } satisfies SessionTranscriptTextMessage
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
      content: [textContentBlock(`Working directory: ${session.cwd}`)],
    }),
  ]
  const activePromptIds: string[] = []
  const agentRowIndexes = new Map<string, number>()
  const toolRowIndexes = new Map<string, number>()

  for (const [index, message] of history.entries()) {
    const promptContent = extractPromptContent(message)

    if (promptContent.length > 0) {
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
          content: promptContent,
        }),
      )
      closePromptTurn({
        activePromptIds,
        agentRowIndexes,
        items,
        message,
      })
      continue
    }

    const sessionUpdate = parseTranscriptSessionUpdate(message)

    if (sessionUpdate) {
      if (sessionUpdate.kind === "toolCall") {
        applyToolCallUpdate({
          items,
          toolRowIndexes,
          session,
          turnId: activePromptIds.at(-1) ?? `${session.id}:orphan`,
          toolCallUpdate: sessionUpdate.toolCallUpdate,
        })
      } else if (sessionUpdate.kind === "agentMessageChunk") {
        applyAgentMessageChunk({
          agentRowIndexes,
          items,
          session,
          text: sessionUpdate.text,
          turnId: activePromptIds.at(-1) ?? `${session.id}:orphan`,
        })
      } else if (sessionUpdate.kind === "unsupported") {
        reportUnsupportedTranscriptMessage(message, sessionUpdate.reason)
      }

      closePromptTurn({
        activePromptIds,
        agentRowIndexes,
        items,
        message,
      })
      continue
    }

    closePromptTurn({
      activePromptIds,
      agentRowIndexes,
      items,
      message,
    })
  }

  if (
    session.lastAgentMessage &&
    !items.some(
      (item) =>
        item.kind === "message" &&
        item.role === "assistant" &&
        item.content.length === 1 &&
        item.content[0]?.type === "text" &&
        item.content[0].text === session.lastAgentMessage,
    )
  ) {
    items.push(
      createTextRow({
        id: `${session.id}:latest`,
        role: "assistant",
        authorName: session.agentName,
        timestampLabel: "Latest",
        content: [textContentBlock(session.lastAgentMessage)],
        streaming: session.activeDaemonSession && session.status === "active",
      }),
    )
  }

  return items
}
