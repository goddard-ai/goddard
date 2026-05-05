import type { DaemonSession, SessionHistoryMessage, SessionHistoryTurn } from "@goddard-ai/sdk"
import { Sigma } from "preact-sigma"

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
export type SessionChatInput = {
  session: DaemonSession
  turns: readonly SessionHistoryTurn[]
}

/** Public state for one session chat transcript owner. */
export type SessionChatState = {
  messages: SessionTranscriptItem[]
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

/** Sigma owner for one session chat transcript and its ACP message accumulation rules. */
export class SessionChat extends Sigma<SessionChatState> {
  /** Assistant row lookup rebuilt with each transcript load; it is derived from ACP turn order. */
  #agentRowIndexes = new Map<string, number>()
  /** Tool row lookup rebuilt with each transcript load; it preserves stable ACP tool identities. */
  #toolRowIndexes = new Map<string, number>()

  constructor(input: SessionChatInput) {
    super({
      messages: [],
    })

    this.loadTranscript(input)
  }

  /** Rebuilds the transcript from the latest daemon session snapshot and ACP turn history. */
  loadTranscript(input: SessionChatInput) {
    this.#agentRowIndexes.clear()
    this.#toolRowIndexes.clear()
    this.messages = [
      createTextRow({
        id: `${input.session.id}:context`,
        role: "system",
        authorName: "System",
        timestampLabel: input.session.status,
        content: [textContentBlock(`Working directory: ${input.session.cwd}`)],
      }),
    ]

    for (const turn of input.turns) {
      const isStreamingTurn = turn.completedAt === null

      for (const [messageIndex, message] of turn.messages.entries()) {
        const promptContent = extractPromptContent(message)

        if (promptContent.length > 0) {
          this.#appendUserPrompt(turn, messageIndex, promptContent)
          continue
        }

        const sessionUpdate = parseTranscriptSessionUpdate(message)

        if (sessionUpdate) {
          if (sessionUpdate.kind === "toolCall") {
            this.#applyToolCallUpdate(input.session, turn.turnId, sessionUpdate.toolCallUpdate)
            continue
          }

          if (sessionUpdate.kind === "agentMessageChunk") {
            this.#applyAgentMessageChunk({
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
    }

    this.#appendLatestDaemonSummary(input.session)
  }

  #appendLatestDaemonSummary(session: DaemonSession) {
    if (
      !session.lastAgentMessage ||
      this.messages.some(
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

    this.messages.push(
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

  #appendUserPrompt(
    turn: SessionHistoryTurn,
    messageIndex: number,
    content: SessionTranscriptContentBlock[],
  ) {
    this.messages.push(
      createTextRow({
        id: `${turn.turnId}:prompt:${messageIndex}`,
        role: "user",
        authorName: "You",
        timestampLabel: "Prompt",
        content,
      }),
    )
  }

  #applyAgentMessageChunk(input: {
    session: DaemonSession
    streaming: boolean
    text: string
    turnId: string
  }) {
    if (input.text.length === 0) {
      return
    }

    const rowKey = `${input.turnId}:agent`
    const rowIndex = this.#agentRowIndexes.get(rowKey)

    if (rowIndex == null) {
      this.#agentRowIndexes.set(
        rowKey,
        this.messages.push(
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

    const existingRow = this.messages[rowIndex]

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

    this.messages[rowIndex] = {
      ...existingRow,
      content: [textContentBlock(`${existingText}${input.text}`)],
      streaming: input.streaming,
    }
  }

  /** Applies one ACP tool update to the transcript, updating an existing card when identities match. */
  #applyToolCallUpdate(
    session: DaemonSession,
    turnId: string,
    toolCallUpdate: ParsedToolCallUpdate,
  ) {
    const rowKey = `${turnId}:tool:${toolCallUpdate.toolCallId}`
    const rowIndex = this.#toolRowIndexes.get(rowKey)

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

      this.#toolRowIndexes.set(rowKey, this.messages.push(toolRow) - 1)
      return
    }

    const existingRow = this.messages[rowIndex]
    if (existingRow.kind !== "toolCall") {
      return
    }

    this.messages[rowIndex] = {
      ...existingRow,
      title: toolCallUpdate.title ?? existingRow.title,
      toolKind: toolCallUpdate.toolKind ?? existingRow.toolKind,
      status: toolCallUpdate.status ?? existingRow.status,
      content: toolCallUpdate.content ?? existingRow.content,
      locations: toolCallUpdate.locations ?? existingRow.locations,
    }
  }
}

export interface SessionChat extends SessionChatState {}
