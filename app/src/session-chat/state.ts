import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse, SessionHistoryTurn } from "@goddard-ai/sdk"

/** UI-facing lifecycle for one prompt turn in the session chat state model. */
export type SessionChatTurnStatus = "running" | "completed" | "failed" | "cancelled" | "stopped"

/** High-level session status exposed to header and action rendering. */
export type SessionChatStatus =
  | "idle"
  | "running"
  | "blocked"
  | "completed"
  | "failed"
  | "cancelled"

/** One event extracted from raw ACP messages for later specialized transcript rows. */
export type SessionChatTurnEvent =
  | {
      kind: "prompt"
      messageIndex: number
      promptRequestId: SessionHistoryTurn["promptRequestId"]
    }
  | {
      kind: "sessionUpdate"
      messageIndex: number
      sessionUpdate: string
    }
  | {
      kind: "planUpdate"
      messageIndex: number
      plan: acp.Plan
    }
  | {
      kind: "permissionRequest"
      messageIndex: number
      request: acp.RequestPermissionRequest
      requestId: string | number
    }
  | {
      kind: "permissionResponse"
      messageIndex: number
      requestId: string | number
    }
  | {
      kind: "turnCompletion"
      messageIndex: number
      completionKind: Exclude<SessionHistoryTurn["completionKind"], null>
      stopReason: SessionHistoryTurn["stopReason"]
    }

/** One normalized session chat turn merged from history and live daemon messages. */
export type SessionChatTurn = SessionHistoryTurn & {
  events: SessionChatTurnEvent[]
  source: "history" | "live" | "merged"
  status: SessionChatTurnStatus
}

/** Derived state facts that later UI tasks can consume without reparsing turns. */
export type SessionChatSummary = {
  activeTurnId: string | null
  pendingPermissionRequest: Extract<SessionChatTurnEvent, { kind: "permissionRequest" }> | null
  status: SessionChatStatus
}

/** Session chat state initialized from history and updated by daemon stream messages. */
export type SessionChatState = {
  connection: GetSessionHistoryResponse["connection"]
  hasMore: boolean
  nextCursor: string | null
  session: DaemonSession
  summary: SessionChatSummary
  turns: SessionChatTurn[]
}

type ApplySessionChatMessageOptions = {
  receivedAt?: string
}

type MessageId = string | number

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

function messageField(message: acp.AnyMessage, key: string) {
  if (!isRecord(message)) {
    return null
  }

  return (message as Record<string, unknown>)[key]
}

function messageId(message: acp.AnyMessage) {
  const id = messageField(message, "id")

  return typeof id === "string" || typeof id === "number" ? id : null
}

function messageMethod(message: acp.AnyMessage) {
  const method = messageField(message, "method")

  return typeof method === "string" ? method : null
}

function messageResult(message: acp.AnyMessage) {
  const result = messageField(message, "result")

  return isRecord(result) ? result : null
}

function messageError(message: acp.AnyMessage) {
  const error = messageField(message, "error")

  return isRecord(error) ? error : null
}

function isPromptMessage(message: acp.AnyMessage) {
  return messageMethod(message) === acp.AGENT_METHODS.session_prompt && messageId(message) !== null
}

function isPermissionRequestMessage(message: acp.AnyMessage) {
  return (
    messageMethod(message) === acp.CLIENT_METHODS.session_request_permission &&
    messageId(message) !== null
  )
}

function sessionUpdate(message: acp.AnyMessage) {
  const params = messageField(message, "params")

  if (messageMethod(message) !== acp.CLIENT_METHODS.session_update || !isRecord(params)) {
    return null
  }

  return isRecord(params.update) ? params.update : null
}

function stableStringify(value: unknown): string {
  if (Array.isArray(value)) {
    return JSON.stringify(value.map((entry) => stableStringify(entry)))
  }

  if (!isRecord(value)) {
    return JSON.stringify(value) ?? String(value)
  }

  const entries = Object.entries(value)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, entryValue]) => [key, stableStringify(entryValue)])

  return JSON.stringify(entries)
}

function messageFingerprint(message: acp.AnyMessage) {
  return stableStringify(message)
}

function turnMessageRank(
  message: acp.AnyMessage,
  promptRequestId: SessionHistoryTurn["promptRequestId"],
) {
  const id = messageId(message)

  if (isPromptMessage(message) && id === promptRequestId) {
    return 0
  }

  if (id === promptRequestId && (messageResult(message) || messageError(message))) {
    return 2
  }

  return 1
}

function insertTurnMessage(
  messages: readonly acp.AnyMessage[],
  message: acp.AnyMessage,
  promptRequestId: SessionHistoryTurn["promptRequestId"],
) {
  const fingerprint = messageFingerprint(message)
  if (messages.some((existingMessage) => messageFingerprint(existingMessage) === fingerprint)) {
    return [...messages]
  }

  return [...messages, message].sort(
    (left, right) =>
      turnMessageRank(left, promptRequestId) - turnMessageRank(right, promptRequestId),
  )
}

function turnStatus(
  turn: Pick<SessionHistoryTurn, "completedAt" | "completionKind" | "stopReason">,
) {
  if (turn.completedAt === null) {
    return "running"
  }

  if (turn.completionKind === "error") {
    return "failed"
  }

  if (turn.stopReason === "cancelled") {
    return "cancelled"
  }

  if (turn.stopReason && turn.stopReason !== "end_turn") {
    return "stopped"
  }

  return "completed"
}

function parsePlanEvent(update: Record<string, unknown>) {
  return update.sessionUpdate === "plan" && Array.isArray(update.entries)
    ? (update as acp.Plan)
    : null
}

function permissionRequestFromMessage(message: acp.AnyMessage) {
  const params = messageField(message, "params")

  if (!isPermissionRequestMessage(message) || !isRecord(params)) {
    return null
  }

  return params as acp.RequestPermissionRequest
}

function buildTurnEvents(turn: SessionHistoryTurn) {
  const permissionRequestIds = new Set<MessageId>()

  for (const message of turn.messages) {
    if (isPermissionRequestMessage(message)) {
      permissionRequestIds.add(messageId(message)!)
    }
  }

  const events: SessionChatTurnEvent[] = []

  for (const [messageIndex, message] of turn.messages.entries()) {
    const id = messageId(message)

    if (isPromptMessage(message) && id === turn.promptRequestId) {
      events.push({
        kind: "prompt",
        messageIndex,
        promptRequestId: turn.promptRequestId,
      })
      continue
    }

    const update = sessionUpdate(message)
    if (update && typeof update.sessionUpdate === "string") {
      const plan = parsePlanEvent(update)

      events.push(
        plan
          ? {
              kind: "planUpdate",
              messageIndex,
              plan,
            }
          : {
              kind: "sessionUpdate",
              messageIndex,
              sessionUpdate: update.sessionUpdate,
            },
      )
      continue
    }

    const permissionRequest = permissionRequestFromMessage(message)
    if (permissionRequest && id !== null) {
      events.push({
        kind: "permissionRequest",
        messageIndex,
        request: permissionRequest,
        requestId: id,
      })
      continue
    }

    if (
      id !== null &&
      permissionRequestIds.has(id) &&
      (messageResult(message) || messageError(message))
    ) {
      events.push({
        kind: "permissionResponse",
        messageIndex,
        requestId: id,
      })
      continue
    }

    if (id === turn.promptRequestId && (messageResult(message) || messageError(message))) {
      events.push({
        kind: "turnCompletion",
        messageIndex,
        completionKind: messageError(message) ? "error" : "result",
        stopReason: extractStopReason(message),
      })
    }
  }

  return events
}

function normalizeTurn(turn: SessionHistoryTurn, source: SessionChatTurn["source"]) {
  return {
    ...turn,
    messages: [...turn.messages],
    events: buildTurnEvents(turn),
    source,
    status: turnStatus(turn),
  } satisfies SessionChatTurn
}

function compareTurns(left: SessionChatTurn, right: SessionChatTurn) {
  if (left.sequence !== right.sequence) {
    return left.sequence - right.sequence
  }

  if (left.startedAt !== right.startedAt) {
    return left.startedAt.localeCompare(right.startedAt)
  }

  return left.turnId.localeCompare(right.turnId)
}

function nextLiveSequence(turns: readonly SessionChatTurn[]) {
  return turns.reduce((sequence, turn) => Math.max(sequence, turn.sequence), -1) + 1
}

function liveTurnId(promptRequestId: SessionHistoryTurn["promptRequestId"]) {
  return `live:${String(promptRequestId)}`
}

function createLiveTurn(
  promptRequestId: SessionHistoryTurn["promptRequestId"],
  receivedAt: string,
  sequence: number,
) {
  return normalizeTurn(
    {
      turnId: liveTurnId(promptRequestId),
      sequence,
      promptRequestId,
      startedAt: receivedAt,
      completedAt: null,
      completionKind: null,
      stopReason: null,
      inboxScope: null,
      inboxHeadline: null,
      messages: [],
    },
    "live",
  )
}

function promptRequestIdFromMessage(message: acp.AnyMessage) {
  const id = messageId(message)

  if (id !== null && isPromptMessage(message)) {
    return id
  }

  if (id !== null && (messageResult(message) || messageError(message))) {
    return id
  }

  return null
}

function newestRunningTurn(turns: readonly SessionChatTurn[]) {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    if (turns[index].completedAt === null) {
      return turns[index]
    }
  }

  return null
}

function resolveTurnForMessage(
  turns: readonly SessionChatTurn[],
  message: acp.AnyMessage,
  receivedAt: string,
) {
  const promptRequestId = promptRequestIdFromMessage(message)
  const existingTurn =
    promptRequestId === null
      ? newestRunningTurn(turns)
      : (turns.find((turn) => turn.promptRequestId === promptRequestId) ?? null)

  if (existingTurn) {
    return existingTurn
  }

  return createLiveTurn(
    promptRequestId ?? `unattributed:${nextLiveSequence(turns)}`,
    receivedAt,
    nextLiveSequence(turns),
  )
}

function extractStopReason(message: acp.AnyMessage) {
  const result = messageResult(message)
  return typeof result?.stopReason === "string"
    ? (result.stopReason as SessionHistoryTurn["stopReason"])
    : null
}

function completeTurnFromMessage(
  turn: SessionChatTurn,
  message: acp.AnyMessage,
  receivedAt: string,
) {
  if (messageId(message) !== turn.promptRequestId) {
    return turn
  }

  if (messageError(message)) {
    return {
      ...turn,
      completedAt: turn.completedAt ?? receivedAt,
      completionKind: "error",
      status: "failed",
    } satisfies SessionChatTurn
  }

  if (messageResult(message)) {
    const stopReason = extractStopReason(message)
    const completedTurn = {
      ...turn,
      completedAt: turn.completedAt ?? receivedAt,
      completionKind: "result",
      stopReason,
    } satisfies SessionChatTurn

    return {
      ...completedTurn,
      status: turnStatus(completedTurn),
    } satisfies SessionChatTurn
  }

  return turn
}

function applyMessageToTurn(turn: SessionChatTurn, message: acp.AnyMessage, receivedAt: string) {
  const nextMessages = insertTurnMessage(turn.messages, message, turn.promptRequestId)
  const nextTurn = completeTurnFromMessage(
    {
      ...turn,
      messages: nextMessages,
      source: turn.source === "history" ? "merged" : turn.source,
    },
    message,
    receivedAt,
  )

  return {
    ...nextTurn,
    events: buildTurnEvents(nextTurn),
    status: turnStatus(nextTurn),
  } satisfies SessionChatTurn
}

function pendingPermissionRequest(turns: readonly SessionChatTurn[]) {
  const resolvedRequestIds = new Set<MessageId>()
  const pendingRequests: Extract<SessionChatTurnEvent, { kind: "permissionRequest" }>[] = []

  for (const turn of turns) {
    for (const event of turn.events) {
      if (event.kind === "permissionResponse") {
        resolvedRequestIds.add(event.requestId)
      } else if (event.kind === "permissionRequest") {
        pendingRequests.push(event)
      }
    }
  }

  for (let index = pendingRequests.length - 1; index >= 0; index -= 1) {
    const request = pendingRequests[index]
    if (!resolvedRequestIds.has(request.requestId)) {
      return request
    }
  }

  return null
}

function sessionChatStatus(
  session: DaemonSession,
  turns: readonly SessionChatTurn[],
  permissionRequest: Extract<SessionChatTurnEvent, { kind: "permissionRequest" }> | null,
): SessionChatStatus {
  if (session.status === "blocked" || session.permissions !== null || permissionRequest) {
    return "blocked"
  }

  if (turns.some((turn) => turn.status === "running")) {
    return "running"
  }

  if (session.status === "error") {
    return "failed"
  }

  if (session.status === "cancelled") {
    return "cancelled"
  }

  if (session.status === "done" || session.status === "archived") {
    return "completed"
  }

  return "idle"
}

function summarizeSessionChat(session: DaemonSession, turns: readonly SessionChatTurn[]) {
  const activeTurn = newestRunningTurn(turns)
  const permissionRequest = pendingPermissionRequest(turns)

  return {
    activeTurnId: activeTurn?.turnId ?? null,
    pendingPermissionRequest: permissionRequest,
    status: sessionChatStatus(session, turns, permissionRequest),
  } satisfies SessionChatSummary
}

function withSummary(state: Omit<SessionChatState, "summary">) {
  const turns = [...state.turns].sort(compareTurns)

  return {
    ...state,
    turns,
    summary: summarizeSessionChat(state.session, turns),
  } satisfies SessionChatState
}

/** Initializes session chat state from one session record and its current history page. */
export function createSessionChatState(input: {
  history: GetSessionHistoryResponse
  session: DaemonSession
}) {
  return withSummary({
    connection: input.history.connection,
    hasMore: input.history.hasMore,
    nextCursor: input.history.nextCursor,
    session: input.session,
    turns: input.history.turns.map((turn) => normalizeTurn(turn, "history")),
  })
}

/** Applies one daemon `session.message` payload from the existing SDK subscription bridge. */
export function applySessionChatMessage(
  state: SessionChatState,
  message: acp.AnyMessage,
  options: ApplySessionChatMessageOptions = {},
) {
  const receivedAt = options.receivedAt ?? new Date().toISOString()
  const targetTurn = resolveTurnForMessage(state.turns, message, receivedAt)
  const nextTurn = applyMessageToTurn(targetTurn, message, receivedAt)
  const replaced = state.turns.some((turn) => turn.turnId === targetTurn.turnId)
  const turns = replaced
    ? state.turns.map((turn) => (turn.turnId === targetTurn.turnId ? nextTurn : turn))
    : [...state.turns, nextTurn]

  return withSummary({
    connection: state.connection,
    hasMore: state.hasMore,
    nextCursor: state.nextCursor,
    session: state.session,
    turns,
  })
}

/** Merges a refreshed history page without dropping live messages already applied locally. */
export function mergeSessionChatHistory(
  state: SessionChatState,
  input: {
    history: GetSessionHistoryResponse
    session: DaemonSession
  },
) {
  let nextState = createSessionChatState(input)

  for (const turn of state.turns) {
    if (turn.source === "history") {
      continue
    }

    for (const message of turn.messages) {
      nextState = applySessionChatMessage(nextState, message, {
        receivedAt: turn.completedAt ?? turn.startedAt,
      })
    }
  }

  return nextState
}

/** Replaces the session record while preserving the current merged turn state. */
export function applySessionChatSession(state: SessionChatState, session: DaemonSession) {
  return withSummary({
    connection: state.connection,
    hasMore: state.hasMore,
    nextCursor: state.nextCursor,
    session,
    turns: state.turns,
  })
}
