import type { DaemonSession, SessionHistoryTurn as SdkSessionHistoryTurn } from "@goddard-ai/sdk"
import type * as acp from "acp-client/protocol"
import hashSum from "hash-sum"
import { isObject } from "radashi"

import { text } from "~/language/text.ts"
import type {
  SessionTranscriptContentBlock,
  SessionTranscriptItem,
  SessionTranscriptPermissionOption,
  SessionTranscriptPermissionOptionKind,
  SessionTranscriptPermissionRequest,
  SessionTranscriptPermissionStatus,
  SessionTranscriptPlanEntry,
  SessionTranscriptPlanEntryPriority,
  SessionTranscriptPlanEntryStatus,
  SessionTranscriptPlanUpdate,
  SessionTranscriptTextMessage,
  SessionTranscriptThought,
  SessionTranscriptToolCall,
  SessionTranscriptToolContent,
  SessionTranscriptToolKind,
  SessionTranscriptToolLocation,
  SessionTranscriptToolStatus,
  SessionTranscriptTurnStop,
  SessionTranscriptWorkDrawer,
} from "~/sessions/models.ts"
import { promptBlocksToTranscriptContent } from "./composer-content.ts"

type SessionHistoryMessage = acp.AnyMessage
type SessionHistoryTurn = Omit<SdkSessionHistoryTurn, "messages"> & {
  messages: SessionHistoryMessage[]
}

type ParsedToolCallUpdate = {
  updateKind: "tool_call" | "tool_call_update"
  toolCallId: string
  title?: string
  toolKind?: SessionTranscriptToolKind
  status?: SessionTranscriptToolStatus
  rawInput?: unknown
  content?: SessionTranscriptToolContent[]
  locations?: SessionTranscriptToolLocation[]
}

type ParsedPlanUpdate = {
  entries: SessionTranscriptPlanEntry[]
  fingerprint: string
}

type MessageId = string | number

type ParsedPermissionResponse =
  | {
      outcome: "selected"
      optionId: string
    }
  | {
      outcome: "cancelled"
    }
  | {
      outcome: "failed"
      error: string | null
    }

type ParsedTranscriptSessionUpdate =
  | {
      kind: "agentMessageChunk"
      text: string
    }
  | {
      kind: "agentThoughtChunk"
      text: string
    }
  | {
      kind: "toolCall"
      toolCallUpdate: ParsedToolCallUpdate
    }
  | {
      kind: "planUpdate"
      planUpdate: ParsedPlanUpdate
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

const PERMISSION_OPTION_KINDS = new Set<SessionTranscriptPermissionOptionKind>([
  "allow_once",
  "allow_always",
  "reject_once",
  "reject_always",
])

const PLAN_ENTRY_PRIORITIES = new Set<SessionTranscriptPlanEntryPriority>(["high", "medium", "low"])

const PLAN_ENTRY_STATUSES = new Set<SessionTranscriptPlanEntryStatus>([
  "pending",
  "in_progress",
  "completed",
])

const TRANSCRIPT_IGNORED_SESSION_UPDATES = new Set([
  "available_commands_update",
  "config_option_update",
  "current_mode_update",
  "session_info_update",
  "usage_update",
])

const TRANSCRIPT_IGNORED_CHUNK_SESSION_UPDATES = new Set(["user_message_chunk"])
const LEADING_SYSTEM_PROMPT_BLOCK_PATTERN =
  /^\s*<system-prompt(?:\s+[^>]*)?>[\s\S]*?<\/system-prompt>\s*/u

function getMessageId(message: unknown) {
  if (!isObject(message)) {
    return null
  }

  const id = (message as Record<string, unknown>).id
  return typeof id === "string" || typeof id === "number" ? id : null
}

function getMessageMethod(message: unknown) {
  if (!isObject(message)) {
    return null
  }

  const method = (message as Record<string, unknown>).method
  return typeof method === "string" ? method : null
}

function hasMethod(
  value: unknown,
  method: "session/prompt" | "session/request_permission" | "session/update",
): value is Record<string, unknown> & {
  method: typeof method
  params?: unknown
} {
  return getMessageMethod(value) === method
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
  if (!hasMethod(message, "session/prompt") || !isObject(message.params)) {
    return []
  }

  const params = message.params as Record<string, unknown>
  return stripLeadingSystemPromptBlocks(promptBlocksToTranscriptContent(params.prompt))
}

function stripLeadingSystemPromptBlocks(content: SessionTranscriptContentBlock[]) {
  const visibleContent: SessionTranscriptContentBlock[] = []
  let isPromptStart = true

  for (const block of content) {
    if (!isPromptStart) {
      visibleContent.push(block)
      continue
    }

    if (block.type !== "text") {
      isPromptStart = false
      visibleContent.push(block)
      continue
    }

    let text = block.text

    while (LEADING_SYSTEM_PROMPT_BLOCK_PATTERN.test(text)) {
      text = text.replace(LEADING_SYSTEM_PROMPT_BLOCK_PATTERN, "")
    }

    if (text.length === 0) {
      continue
    }

    isPromptStart = false
    visibleContent.push({
      ...block,
      text,
    })
  }

  return visibleContent
}

function extractSessionUpdate(message: SessionHistoryMessage) {
  if (!hasMethod(message, "session/update") || !isObject(message.params)) {
    return null
  }

  const params = message.params as Record<string, unknown>
  return isObject(params.update) ? (params.update as Record<string, unknown>) : null
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

function extractPermissionOptionKind(value: unknown) {
  return typeof value === "string" &&
    PERMISSION_OPTION_KINDS.has(value as SessionTranscriptPermissionOptionKind)
    ? (value as SessionTranscriptPermissionOptionKind)
    : null
}

function extractPlanEntryPriority(value: unknown) {
  return typeof value === "string" &&
    PLAN_ENTRY_PRIORITIES.has(value as SessionTranscriptPlanEntryPriority)
    ? (value as SessionTranscriptPlanEntryPriority)
    : null
}

function extractPlanEntryStatus(value: unknown) {
  return typeof value === "string" &&
    PLAN_ENTRY_STATUSES.has(value as SessionTranscriptPlanEntryStatus)
    ? (value as SessionTranscriptPlanEntryStatus)
    : null
}

function buildPlanFingerprint(entries: readonly SessionTranscriptPlanEntry[]) {
  return hashSum(entries)
}

function extractPlanEntries(value: unknown): SessionTranscriptPlanEntry[] | null {
  if (!Array.isArray(value)) {
    return null
  }

  const entries: SessionTranscriptPlanEntry[] = []

  for (const entry of value) {
    const record = isObject(entry) ? (entry as Record<string, unknown>) : null
    if (!record || typeof record.content !== "string") {
      return null
    }

    const priority = extractPlanEntryPriority(record.priority)
    const status = extractPlanEntryStatus(record.status)

    if (!priority || !status) {
      return null
    }

    entries.push({
      content: record.content,
      priority,
      status,
    })
  }

  return entries
}

function extractPlanUpdate(update: Record<string, unknown>): ParsedPlanUpdate | null {
  if (update.sessionUpdate !== "plan") {
    return null
  }

  const entries = extractPlanEntries(update.entries)

  if (!entries) {
    return null
  }

  return {
    entries,
    fingerprint: buildPlanFingerprint(entries),
  }
}

function extractPermissionOptions(value: unknown): SessionTranscriptPermissionOption[] {
  if (!Array.isArray(value)) {
    return []
  }

  const options: SessionTranscriptPermissionOption[] = []

  for (const option of value) {
    if (
      !isObject(option) ||
      typeof (option as Record<string, unknown>).optionId !== "string" ||
      typeof (option as Record<string, unknown>).name !== "string"
    ) {
      continue
    }

    const record = option as Record<string, unknown>
    const kind = extractPermissionOptionKind(record.kind)

    if (!kind) {
      continue
    }

    options.push({
      optionId: record.optionId as string,
      name: record.name as string,
      kind,
    })
  }

  return options
}

function extractToolCallLocations(value: unknown): SessionTranscriptToolLocation[] {
  if (!Array.isArray(value)) {
    return []
  }

  const locations: SessionTranscriptToolLocation[] = []

  for (const location of value) {
    const record = isObject(location) ? (location as Record<string, unknown>) : null
    if (!record || typeof record.path !== "string") {
      continue
    }

    locations.push({
      path: record.path,
      line: typeof record.line === "number" ? record.line : null,
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
    const record = isObject(item) ? (item as Record<string, unknown>) : null
    if (!record || typeof record.type !== "string") {
      continue
    }

    if (record.type === "content") {
      content.push({
        type: "content",
        text: textFromContentBlocks(record.content),
      })
      continue
    }

    if (record.type === "diff") {
      content.push({
        type: "diff",
        path: typeof record.path === "string" ? record.path : null,
        oldText: typeof record.oldText === "string" ? record.oldText : null,
        newText: typeof record.newText === "string" ? record.newText : null,
      })
      continue
    }

    if (record.type === "terminal" && typeof record.terminalId === "string") {
      content.push({
        type: "terminal",
        terminalId: record.terminalId,
      })
    }
  }

  return content
}

function formatPermissionContext(value: unknown) {
  if (value == null) {
    return null
  }

  if (typeof value === "string") {
    const text = value.trim()
    return text.length > 0 ? text : null
  }

  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function extractPermissionRequest(message: SessionHistoryMessage) {
  if (!hasMethod(message, "session/request_permission") || !isObject(message.params)) {
    return null
  }

  const requestId = getMessageId(message)
  const params = message.params as Record<string, unknown>
  const toolCall = isObject(params.toolCall) ? (params.toolCall as Record<string, unknown>) : null

  if (requestId === null || !toolCall || typeof toolCall.toolCallId !== "string") {
    return null
  }

  const toolKind = extractToolKind(toolCall.kind) ?? "other"

  return {
    requestId,
    title:
      typeof toolCall.title === "string" && toolCall.title.trim().length > 0
        ? toolCall.title.trim()
        : fallbackToolTitle(toolKind),
    toolKind,
    context: formatPermissionContext(toolCall.rawInput),
    locations: extractToolCallLocations(toolCall.locations),
    options: extractPermissionOptions(params.options),
  }
}

function extractPermissionResponse(
  message: SessionHistoryMessage,
): ParsedPermissionResponse | null {
  if (!isObject(message)) {
    return null
  }

  const errorText = extractMessageErrorText(message)

  if (errorText) {
    return {
      outcome: "failed",
      error: errorText,
    }
  }

  const messageRecord = message as Record<string, unknown>
  const result = "result" in messageRecord ? messageRecord.result : null

  const resultRecord = isObject(result) ? (result as Record<string, unknown>) : null
  if (!resultRecord || !isObject(resultRecord.outcome)) {
    return null
  }

  const outcome = resultRecord.outcome as Record<string, unknown>

  if (outcome.outcome === "cancelled") {
    return {
      outcome: "cancelled",
    }
  }

  if (outcome.outcome === "selected" && typeof outcome.optionId === "string") {
    return {
      outcome: "selected",
      optionId: outcome.optionId,
    }
  }

  return null
}

/** Correlates ACP permission response frames to the request rows rendered for one turn. */
function buildPermissionResponsesByRequestId(
  messages: readonly SessionHistoryMessage[],
): Map<MessageId, ParsedPermissionResponse> {
  const requestIds = new Set<MessageId>()
  const responses = new Map<MessageId, ParsedPermissionResponse>()

  for (const message of messages) {
    const request = extractPermissionRequest(message)

    if (request) {
      requestIds.add(request.requestId)
    }
  }

  for (const message of messages) {
    const id = getMessageId(message)

    if (id === null || !requestIds.has(id)) {
      continue
    }

    const response = extractPermissionResponse(message)

    if (response) {
      responses.set(id, response)
    }
  }

  return responses
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

  if ("rawInput" in update) {
    toolCallUpdate.rawInput = update.rawInput ?? null
  }

  if ("locations" in update) {
    toolCallUpdate.locations = extractToolCallLocations(update.locations)
  }

  return toolCallUpdate
}

function extractTextChunk(update: Record<string, unknown>, sessionUpdate: string) {
  if (update.sessionUpdate !== sessionUpdate) {
    return undefined
  }

  if (
    !isObject(update.content) ||
    (update.content as Record<string, unknown>).type !== "text" ||
    typeof (update.content as Record<string, unknown>).text !== "string"
  ) {
    return null
  }

  return (update.content as Record<string, unknown>).text as string
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

  const agentMessageChunkText = extractTextChunk(update, "agent_message_chunk")

  if (agentMessageChunkText !== undefined) {
    if (agentMessageChunkText === null) {
      return {
        kind: "unsupported",
        reason: "agent_message_chunk only supports text content.",
      }
    }

    return {
      kind: "agentMessageChunk",
      text: agentMessageChunkText,
    }
  }

  const agentThoughtChunkText = extractTextChunk(update, "agent_thought_chunk")

  if (agentThoughtChunkText !== undefined) {
    if (agentThoughtChunkText === null) {
      return {
        kind: "unsupported",
        reason: "agent_thought_chunk only supports text content.",
      }
    }

    return {
      kind: "agentThoughtChunk",
      text: agentThoughtChunkText,
    }
  }

  if (TRANSCRIPT_IGNORED_CHUNK_SESSION_UPDATES.has(update.sessionUpdate)) {
    return {
      kind: "ignored",
    }
  }

  if (update.sessionUpdate === "plan") {
    const planUpdate = extractPlanUpdate(update)

    if (!planUpdate) {
      return {
        kind: "unsupported",
        reason: "plan update is missing valid plan entries.",
      }
    }

    return {
      kind: "planUpdate",
      planUpdate,
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

function createThoughtRow(input: Omit<SessionTranscriptThought, "kind">) {
  return {
    kind: "thought",
    ...input,
  } satisfies SessionTranscriptThought
}

function extractMessageErrorText(message: SessionHistoryMessage) {
  const error = isObject(message) ? (message as Record<string, unknown>)["error"] : null

  if (!isObject(error)) {
    return null
  }

  const record = error as Record<string, unknown>
  return typeof record.message === "string" && record.message.trim().length > 0
    ? record.message.trim()
    : null
}

function findPermissionOption(
  options: readonly SessionTranscriptPermissionOption[],
  optionId: string | null,
) {
  if (!optionId) {
    return null
  }

  return options.find((option) => option.optionId === optionId) ?? null
}

function resolvePermissionStatus(
  options: readonly SessionTranscriptPermissionOption[],
  response: ParsedPermissionResponse | null,
): SessionTranscriptPermissionStatus {
  if (!response) {
    return "pending"
  }

  if (response.outcome === "failed") {
    return "failed"
  }

  if (response.outcome === "cancelled") {
    return "cancelled"
  }

  const option = findPermissionOption(options, response.optionId)

  if (!option) {
    return "resolved"
  }

  return option.kind.startsWith("reject_") ? "denied" : "allowed"
}

function resolvePermissionSelectedOptionId(response: ParsedPermissionResponse | null) {
  return response?.outcome === "selected" ? response.optionId : null
}

function resolvePermissionError(response: ParsedPermissionResponse | null) {
  return response?.outcome === "failed" ? response.error : null
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
  return isObject(message) && (message as Record<string, unknown>)["id"] === turn.promptRequestId
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
      title: text.interrupted,
      reason: session.errorMessage ?? "No turn completion was recorded",
      timestamp: null,
    } satisfies SessionTranscriptTurnStop
  }

  if (turn.completionKind === "error") {
    return {
      kind: "turnStop",
      id: `${turn.turnId}:stop`,
      status: "failed",
      title: text.failed,
      reason: extractTurnFailureReason(turn, session),
      timestamp: turn.completedAt,
    } satisfies SessionTranscriptTurnStop
  }

  if (turn.stopReason === "cancelled") {
    return {
      kind: "turnStop",
      id: `${turn.turnId}:stop`,
      status: "cancelled",
      title: text.cancelled,
      reason: formatStopReason(turn.stopReason),
      timestamp: turn.completedAt,
    } satisfies SessionTranscriptTurnStop
  }

  if (turn.stopReason && turn.stopReason !== "end_turn") {
    return {
      kind: "turnStop",
      id: `${turn.turnId}:stop`,
      status: "stopped",
      title: text.stopped,
      reason: formatStopReason(turn.stopReason),
      timestamp: turn.completedAt,
    } satisfies SessionTranscriptTurnStop
  }

  return null
}

function formatTurnDuration(startedAt: string, completedAt: string | null) {
  const startedTime = Date.parse(startedAt)
  const completedTime = completedAt ? Date.parse(completedAt) : Date.now()

  if (!Number.isFinite(startedTime) || !Number.isFinite(completedTime)) {
    return null
  }

  const totalSeconds = Math.max(0, Math.round((completedTime - startedTime) / 1000))
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60

  if (minutes > 0) {
    return `${minutes}m ${seconds.toString().padStart(2, "0")}s`
  }

  return `${seconds}s`
}

/** Builds one session chat transcript from session state and ACP message history. */
export function buildSessionChatTranscript(input: SessionChatTranscriptInput) {
  const agentRowIndexes = new Map<string, number>()
  let previousPlanFingerprint: string | null = null
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
    turnWorkItems: Array<SessionTranscriptThought | SessionTranscriptToolCall>,
    turnToolRowIndexes: Map<string, number>,
    toolCallUpdate: ParsedToolCallUpdate,
  ) {
    const rowKey = `${turnId}:tool:${toolCallUpdate.toolCallId}`
    const rowIndex = turnToolRowIndexes.get(rowKey)

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
        rawInput: toolCallUpdate.rawInput ?? null,
        content: toolCallUpdate.content ?? [],
        locations: toolCallUpdate.locations ?? [],
      }

      turnToolRowIndexes.set(rowKey, turnWorkItems.push(toolRow) - 1)
      return
    }

    const existingRow = turnWorkItems[rowIndex]
    if (existingRow.kind !== "toolCall") {
      return
    }

    turnWorkItems[rowIndex] = {
      ...existingRow,
      title: toolCallUpdate.title ?? existingRow.title,
      toolKind: toolCallUpdate.toolKind ?? existingRow.toolKind,
      status: toolCallUpdate.status ?? existingRow.status,
      rawInput: toolCallUpdate.rawInput ?? existingRow.rawInput,
      content: toolCallUpdate.content ?? existingRow.content,
      locations: toolCallUpdate.locations ?? existingRow.locations,
    }
  }

  function appendThoughtChunk(
    session: DaemonSession,
    turnId: string,
    messageIndex: number,
    turnWorkItems: Array<SessionTranscriptThought | SessionTranscriptToolCall>,
    text: string,
  ) {
    if (text.length === 0) {
      return
    }

    const previousItem = turnWorkItems.at(-1)
    if (previousItem?.kind === "thought") {
      turnWorkItems[turnWorkItems.length - 1] = {
        ...previousItem,
        text: `${previousItem.text}${text}`,
      }
      return
    }

    turnWorkItems.push(
      createThoughtRow({
        id: `${turnId}:thought:${messageIndex}`,
        authorName: session.agentName,
        timestampLabel: "Thought",
        text,
      }),
    )
  }

  function insertTurnWorkDrawer(
    turn: SessionHistoryTurn,
    turnWorkItems: readonly (SessionTranscriptThought | SessionTranscriptToolCall)[],
  ) {
    if (turn.completedAt === null || turnWorkItems.length === 0) {
      return
    }

    const duration = formatTurnDuration(turn.startedAt, turn.completedAt)
    const workDrawer: SessionTranscriptWorkDrawer = {
      kind: "workDrawer",
      id: `${turn.turnId}:work`,
      title: `${turn.completedAt === null ? "Working" : "Worked"}${
        duration ? ` for ${duration}` : ""
      }`,
      expandedByDefault: turn.completedAt === null,
      items: turnWorkItems,
    }
    const agentRowIndex = agentRowIndexes.get(`${turn.turnId}:agent`)

    if (agentRowIndex == null) {
      messages.push(workDrawer)
      return
    }

    messages.splice(agentRowIndex, 0, workDrawer)

    for (const [rowKey, rowIndex] of agentRowIndexes) {
      if (rowIndex >= agentRowIndex) {
        agentRowIndexes.set(rowKey, rowIndex + 1)
      }
    }
  }

  function appendPermissionRequest(
    session: DaemonSession,
    turnId: string,
    messageIndex: number,
    request: NonNullable<ReturnType<typeof extractPermissionRequest>>,
    response: ParsedPermissionResponse | null,
  ) {
    const permissionRow: SessionTranscriptPermissionRequest = {
      kind: "permissionRequest",
      id: `${turnId}:permission:${String(request.requestId)}:${messageIndex}`,
      requestId: request.requestId,
      authorName: session.agentName,
      timestampLabel: "Permission",
      title: request.title,
      toolKind: request.toolKind,
      status: resolvePermissionStatus(request.options, response),
      context: request.context,
      locations: request.locations,
      options: request.options,
      selectedOptionId: resolvePermissionSelectedOptionId(response),
      error: resolvePermissionError(response),
    }

    messages.push(permissionRow)
  }

  function appendPlanUpdate(
    session: DaemonSession,
    turnId: string,
    messageIndex: number,
    planUpdate: ParsedPlanUpdate,
  ) {
    if (previousPlanFingerprint === planUpdate.fingerprint) {
      return
    }

    previousPlanFingerprint = planUpdate.fingerprint

    const completedCount = planUpdate.entries.filter((entry) => entry.status === "completed").length
    const planRow: SessionTranscriptPlanUpdate = {
      kind: "planUpdate",
      id: `${turnId}:plan:${messageIndex}`,
      authorName: session.agentName,
      timestampLabel: "Plan",
      title:
        planUpdate.entries.length === 0
          ? "Plan cleared"
          : `Plan updated · ${completedCount}/${planUpdate.entries.length} complete`,
      entries: planUpdate.entries,
    }

    messages.push(planRow)
  }

  for (const [turnIndex, turn] of input.turns.entries()) {
    const isStreamingTurn = turn.completedAt === null
    const permissionResponsesByRequestId = buildPermissionResponsesByRequestId(turn.messages)
    const turnWorkItems: Array<SessionTranscriptThought | SessionTranscriptToolCall> =
      isStreamingTurn
        ? (messages as Array<SessionTranscriptThought | SessionTranscriptToolCall>)
        : []
    const turnToolRowIndexes = new Map<string, number>()

    for (const [messageIndex, message] of turn.messages.entries()) {
      const promptContent = extractPromptContent(message)

      if (promptContent.length > 0) {
        appendUserPrompt(turn, messageIndex, promptContent)
        continue
      }

      const permissionRequest = extractPermissionRequest(message)

      if (permissionRequest) {
        appendPermissionRequest(
          input.session,
          turn.turnId,
          messageIndex,
          permissionRequest,
          permissionResponsesByRequestId.get(permissionRequest.requestId) ?? null,
        )
        continue
      }

      const sessionUpdate = parseTranscriptSessionUpdate(message)

      if (sessionUpdate) {
        if (sessionUpdate.kind === "toolCall") {
          applyToolCallUpdate(
            input.session,
            turn.turnId,
            turnWorkItems,
            turnToolRowIndexes,
            sessionUpdate.toolCallUpdate,
          )
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

        if (sessionUpdate.kind === "agentThoughtChunk") {
          appendThoughtChunk(
            input.session,
            turn.turnId,
            messageIndex,
            turnWorkItems,
            sessionUpdate.text,
          )
          continue
        }

        if (sessionUpdate.kind === "planUpdate") {
          appendPlanUpdate(input.session, turn.turnId, messageIndex, sessionUpdate.planUpdate)
          continue
        }

        if (sessionUpdate.kind === "ignored") {
          continue
        }

        reportUnsupportedTranscriptMessage(message, sessionUpdate.reason)
        continue
      }
    }

    insertTurnWorkDrawer(turn, turnWorkItems)

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
