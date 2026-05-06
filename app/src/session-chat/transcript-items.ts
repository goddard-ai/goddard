import type { DaemonSession, SessionHistoryMessage, SessionHistoryTurn } from "@goddard-ai/sdk"

import type {
  SessionTranscriptContentBlock,
  SessionTranscriptItem,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
  SessionTranscriptToolContent,
  SessionTranscriptToolKind,
  SessionTranscriptToolLocation,
  SessionTranscriptToolStatus,
  SessionTranscriptTurnStop,
} from "~/sessions/models.ts"
import { promptBlocksToTranscriptContent } from "./composer-content.ts"

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

/** Runtime inputs used to rebuild one session chat transcript. */
export type SessionChatTranscriptInput = {
  session: DaemonSession
  turns: readonly SessionHistoryTurn[]
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
): value is Record<string, unknown> & {
  method: typeof method
  params?: unknown
} {
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

function fallbackToolTitle(toolKind: SessionTranscriptToolKind) {
  if (toolKind === "switch_mode") {
    return "Switch mode"
  }

  return `${toolKind.slice(0, 1).toUpperCase()}${toolKind.slice(1)} tool`
}

function createTextRow(input: Omit<SessionTranscriptTextMessage, "kind">) {
  return {
    kind: "message",
    ...input,
  } satisfies SessionTranscriptTextMessage
}

function extractMessageErrorText(message: SessionHistoryMessage) {
  const error = isRecord(message) ? (message as Record<string, unknown>)["error"] : null

  if (!isRecord(error)) {
    return null
  }

  return typeof error.message === "string" && error.message.trim().length > 0
    ? error.message.trim()
    : null
}

function formatStopReason(stopReason: SessionHistoryTurn["stopReason"]) {
  switch (stopReason) {
    case "max_tokens":
      return "Reached the token limit"
    case "max_turn_requests":
      return "Reached the turn limit"
    case "refusal":
      return "Agent refused"
    case "cancelled":
      return "Cancelled by request"
    default:
      return null
  }
}

function extractTurnFailureReason(turn: SessionHistoryTurn, session: DaemonSession) {
  for (const message of turn.messages) {
    if (messageIdMatchesPrompt(turn, message)) {
      const errorText = extractMessageErrorText(message)

      if (errorText) {
        return errorText
      }
    }
  }

  return session.errorMessage
}

function messageIdMatchesPrompt(turn: SessionHistoryTurn, message: SessionHistoryMessage) {
  return isRecord(message) && (message as Record<string, unknown>)["id"] === turn.promptRequestId
}

function createTurnStopRow(session: DaemonSession, turn: SessionHistoryTurn) {
  if (turn.completedAt === null) {
    if (session.activeDaemonSession) {
      return null
    }

    return {
      kind: "turnStop",
      id: `${turn.turnId}:stop`,
      status: "interrupted",
      title: "Interrupted",
      reason: session.errorMessage ?? "No turn completion was recorded",
      timestamp: null,
    } satisfies SessionTranscriptTurnStop
  }

  if (turn.completionKind === "error") {
    return {
      kind: "turnStop",
      id: `${turn.turnId}:stop`,
      status: "failed",
      title: "Failed",
      reason: extractTurnFailureReason(turn, session),
      timestamp: turn.completedAt,
    } satisfies SessionTranscriptTurnStop
  }

  if (turn.stopReason === "cancelled") {
    return {
      kind: "turnStop",
      id: `${turn.turnId}:stop`,
      status: "cancelled",
      title: "Cancelled",
      reason: formatStopReason(turn.stopReason),
      timestamp: turn.completedAt,
    } satisfies SessionTranscriptTurnStop
  }

  if (turn.stopReason && turn.stopReason !== "end_turn") {
    return {
      kind: "turnStop",
      id: `${turn.turnId}:stop`,
      status: "stopped",
      title: "Stopped",
      reason: formatStopReason(turn.stopReason),
      timestamp: turn.completedAt,
    } satisfies SessionTranscriptTurnStop
  }

  return {
    kind: "turnStop",
    id: `${turn.turnId}:stop`,
    status: "completed",
    title: "Completed",
    reason: null,
    timestamp: turn.completedAt,
  } satisfies SessionTranscriptTurnStop
}

/** Builds one session chat transcript from session state and ACP message history. */
export function buildSessionChatTranscript(input: SessionChatTranscriptInput) {
  const agentRowIndexes = new Map<string, number>()
  const toolRowIndexes = new Map<string, number>()
  const messages: SessionTranscriptItem[] = [
    createTextRow({
      id: `${input.session.id}:context`,
      role: "system",
      authorName: "System",
      timestampLabel: input.session.status,
      content: [textContentBlock(`Working directory: ${input.session.cwd}`)],
    }),
  ]

  function appendLatestDaemonSummary(session: DaemonSession) {
    if (
      !session.lastAgentMessage ||
      messages.some(
        (item) =>
          item.kind === "message" &&
          item.role === "assistant" &&
          item.content.length === 1 &&
          item.content[0]?.type === "text" &&
          item.content[0].text === session.lastAgentMessage,
      )
    ) {
      return
    }

    messages.push(
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

  function appendUserPrompt(
    turn: SessionHistoryTurn,
    messageIndex: number,
    content: SessionTranscriptContentBlock[],
  ) {
    messages.push(
      createTextRow({
        id: `${turn.turnId}:prompt:${messageIndex}`,
        role: "user",
        authorName: "You",
        timestampLabel: "Prompt",
        content,
      }),
    )
  }

  function applyAgentMessageChunk(input: {
    session: DaemonSession
    streaming: boolean
    text: string
    turnId: string
  }) {
    if (input.text.length === 0) {
      return
    }

    const rowKey = `${input.turnId}:agent`
    const rowIndex = agentRowIndexes.get(rowKey)

    if (rowIndex == null) {
      agentRowIndexes.set(
        rowKey,
        messages.push(
          createTextRow({
            id: rowKey,
            role: "assistant",
            authorName: input.session.agentName,
            timestampLabel: "Update",
            content: [textContentBlock(input.text)],
            streaming: input.streaming,
          }),
        ) - 1,
      )
      return
    }

    const existingRow = messages[rowIndex]

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

    messages[rowIndex] = {
      ...existingRow,
      content: [textContentBlock(`${existingText}${input.text}`)],
      streaming: input.streaming,
    }
  }

  function applyToolCallUpdate(
    session: DaemonSession,
    turnId: string,
    toolCallUpdate: ParsedToolCallUpdate,
  ) {
    const rowKey = `${turnId}:tool:${toolCallUpdate.toolCallId}`
    const rowIndex = toolRowIndexes.get(rowKey)

    if (rowIndex == null) {
      const toolKind = toolCallUpdate.toolKind ?? "other"
      const toolRow: SessionTranscriptToolCall = {
        kind: "toolCall",
        id: rowKey,
        toolCallId: toolCallUpdate.toolCallId,
        authorName: session.agentName,
        timestampLabel: "Tool",
        title: toolCallUpdate.title ?? fallbackToolTitle(toolKind),
        toolKind,
        status:
          toolCallUpdate.status ??
          (toolCallUpdate.updateKind === "tool_call" ? "in_progress" : "pending"),
        content: toolCallUpdate.content ?? [],
        locations: toolCallUpdate.locations ?? [],
      }

      toolRowIndexes.set(rowKey, messages.push(toolRow) - 1)
      return
    }

    const existingRow = messages[rowIndex]
    if (existingRow.kind !== "toolCall") {
      return
    }

    messages[rowIndex] = {
      ...existingRow,
      title: toolCallUpdate.title ?? existingRow.title,
      toolKind: toolCallUpdate.toolKind ?? existingRow.toolKind,
      status: toolCallUpdate.status ?? existingRow.status,
      content: toolCallUpdate.content ?? existingRow.content,
      locations: toolCallUpdate.locations ?? existingRow.locations,
    }
  }

  for (const [turnIndex, turn] of input.turns.entries()) {
    const isStreamingTurn = turn.completedAt === null

    for (const [messageIndex, message] of turn.messages.entries()) {
      const promptContent = extractPromptContent(message)

      if (promptContent.length > 0) {
        appendUserPrompt(turn, messageIndex, promptContent)
        continue
      }

      const sessionUpdate = parseTranscriptSessionUpdate(message)

      if (sessionUpdate) {
        if (sessionUpdate.kind === "toolCall") {
          applyToolCallUpdate(input.session, turn.turnId, sessionUpdate.toolCallUpdate)
          continue
        }

        if (sessionUpdate.kind === "agentMessageChunk") {
          applyAgentMessageChunk({
            session: input.session,
            streaming: isStreamingTurn,
            text: sessionUpdate.text,
            turnId: turn.turnId,
          })
          continue
        }

        if (sessionUpdate.kind === "ignored") {
          continue
        }

        reportUnsupportedTranscriptMessage(message, sessionUpdate.reason)
        continue
      }
    }

    if (turnIndex === input.turns.length - 1) {
      appendLatestDaemonSummary(input.session)
    }

    const turnStopRow = createTurnStopRow(input.session, turn)

    if (turnStopRow) {
      messages.push(turnStopRow)
    }
  }

  if (input.turns.length === 0) {
    appendLatestDaemonSummary(input.session)
  }

  return messages
}
