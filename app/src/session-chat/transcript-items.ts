import type { DaemonSession, SessionHistoryMessage, SessionHistoryTurn } from "@goddard-ai/sdk"
import hashSum from "hash-sum"

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
])

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

function getMessageId(message: unknown) {
  if (!isRecord(message)) {
    return null
  }

  const id = message.id
  return typeof id === "string" || typeof id === "number" ? id : null
}

function getMessageMethod(message: unknown) {
  if (!isRecord(message)) {
    return null
  }

  return typeof message.method === "string" ? message.method : null
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
    if (!isRecord(entry) || typeof entry.content !== "string") {
      return null
    }

    const priority = extractPlanEntryPriority(entry.priority)
    const status = extractPlanEntryStatus(entry.status)

    if (!priority || !status) {
      return null
    }

    entries.push({
      content: entry.content,
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
      !isRecord(option) ||
      typeof option.optionId !== "string" ||
      typeof option.name !== "string"
    ) {
      continue
    }

    const kind = extractPermissionOptionKind(option.kind)

    if (!kind) {
      continue
    }

    options.push({
      optionId: option.optionId,
      name: option.name,
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
  if (!hasMethod(message, "session/request_permission") || !isRecord(message.params)) {
    return null
  }

  const requestId = getMessageId(message)
  const toolCall = isRecord(message.params.toolCall) ? message.params.toolCall : null

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
    options: extractPermissionOptions(message.params.options),
  }
}

function extractPermissionResponse(
  message: SessionHistoryMessage,
): ParsedPermissionResponse | null {
  if (!isRecord(message)) {
    return null
  }

  const errorText = extractMessageErrorText(message)

  if (errorText) {
    return {
      outcome: "failed",
      error: errorText,
    }
  }

  const result = "result" in message ? message.result : null

  if (!isRecord(result) || !isRecord(result.outcome)) {
    return null
  }

  const outcome = result.outcome

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

function extractMessageErrorText(message: SessionHistoryMessage) {
  const error = isRecord(message) ? (message as Record<string, unknown>)["error"] : null

  if (!isRecord(error)) {
    return null
  }

  return typeof error.message === "string" && error.message.trim().length > 0
    ? error.message.trim()
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
