import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse, SessionHistoryTurn } from "@goddard-ai/sdk"
import hashSum from "hash-sum"
import { castDraft } from "immer"
import { Sigma, type Immutable } from "preact-sigma"

import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "~/sessions/display.ts"
import { buildSessionChatTranscript } from "./transcript-items.ts"

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

/** Derived state facts consumed by the session chat view and later transcript rows. */
export type SessionChatSummary = {
  activeTurnId: string | null
  contextUsage: SessionChatContextUsage | null
  pendingPermissionRequest: Extract<SessionChatTurnEvent, { kind: "permissionRequest" }> | null
  showThinkingLabel: boolean
  status: SessionChatStatus
}

/** Latest model context window usage reported by the agent. */
export type SessionChatContextUsage = {
  size: number
  used: number
}

/** Reactive session chat state initialized from history and updated by daemon stream messages. */
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

type SessionPermissionRequest = Extract<SessionChatTurnEvent, { kind: "permissionRequest" }>

type QueuedSessionChatMessage = {
  message: acp.AnyMessage
  receivedAt: string
}

const liveChunkBatchIntervalMs = 33

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

function getMessageField(message: acp.AnyMessage, key: string) {
  if (!isRecord(message)) {
    return null
  }

  return (message as Record<string, unknown>)[key]
}

function getMessageId(message: acp.AnyMessage) {
  const id = getMessageField(message, "id")

  return typeof id === "string" || typeof id === "number" ? id : null
}

function getMessageMethod(message: acp.AnyMessage) {
  const method = getMessageField(message, "method")

  return typeof method === "string" ? method : null
}

function getMessageResult(message: acp.AnyMessage) {
  const result = getMessageField(message, "result")

  return isRecord(result) ? result : null
}

function getMessageError(message: acp.AnyMessage) {
  const error = getMessageField(message, "error")

  return isRecord(error) ? error : null
}

function isPromptMessage(message: acp.AnyMessage) {
  return (
    getMessageMethod(message) === acp.AGENT_METHODS.session_prompt && getMessageId(message) !== null
  )
}

function isPermissionRequestMessage(message: acp.AnyMessage) {
  return (
    getMessageMethod(message) === acp.CLIENT_METHODS.session_request_permission &&
    getMessageId(message) !== null
  )
}

function getSessionUpdate(message: acp.AnyMessage) {
  const params = getMessageField(message, "params")

  if (getMessageMethod(message) !== acp.CLIENT_METHODS.session_update || !isRecord(params)) {
    return null
  }

  return isRecord(params.update) ? params.update : null
}

function isTextAgentMessageChunk(message: acp.AnyMessage) {
  const update = getSessionUpdate(message)

  return (
    update?.sessionUpdate === "agent_message_chunk" &&
    isRecord(update.content) &&
    update.content.type === "text" &&
    typeof update.content.text === "string"
  )
}

function getTextAgentMessageChunkText(message: acp.AnyMessage) {
  const update = getSessionUpdate(message)

  return update?.sessionUpdate === "agent_message_chunk" &&
    isRecord(update.content) &&
    update.content.type === "text" &&
    typeof update.content.text === "string"
    ? update.content.text
    : null
}

function createAgentMessageChunk(sessionId: string, text: string) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text },
      },
    },
  } satisfies acp.AnyMessage
}

function buildMessageFingerprint(message: acp.AnyMessage) {
  return hashSum(message)
}

function getTurnMessageRank(
  message: acp.AnyMessage,
  promptRequestId: SessionHistoryTurn["promptRequestId"],
) {
  const id = getMessageId(message)

  if (isPromptMessage(message) && id === promptRequestId) {
    return 0
  }

  if (id === promptRequestId && (getMessageResult(message) || getMessageError(message))) {
    return 2
  }

  return 1
}

function resolveTurnStatus(
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

function parseContextUsage(update: Record<string, unknown>): SessionChatContextUsage | null {
  if (
    update.sessionUpdate !== "usage_update" ||
    typeof update.size !== "number" ||
    typeof update.used !== "number" ||
    !Number.isFinite(update.size) ||
    !Number.isFinite(update.used) ||
    update.size <= 0 ||
    update.used < 0
  ) {
    return null
  }

  return {
    size: update.size,
    used: update.used,
  }
}

function getToolCallUpdateStatus(message: acp.AnyMessage) {
  const update = getSessionUpdate(message)

  if (
    !update ||
    (update.sessionUpdate !== "tool_call" && update.sessionUpdate !== "tool_call_update") ||
    typeof update.toolCallId !== "string"
  ) {
    return null
  }

  return {
    status: typeof update.status === "string" ? update.status : null,
    toolCallId: update.toolCallId,
    updateKind: update.sessionUpdate,
  }
}

function hasActiveToolCall(turn: SessionChatTurn) {
  const toolStatuses = new Map<string, string>()

  for (const message of turn.messages) {
    const update = getToolCallUpdateStatus(message)

    if (!update) {
      continue
    }

    toolStatuses.set(
      update.toolCallId,
      update.status ?? (update.updateKind === "tool_call" ? "in_progress" : "pending"),
    )
  }

  return [...toolStatuses.values()].some(
    (status) => status === "pending" || status === "in_progress",
  )
}

function shouldShowThinkingLabel(input: {
  activeTurn: SessionChatTurn | null
  permissionRequest: SessionPermissionRequest | null
  status: SessionChatStatus
}) {
  return (
    input.status === "running" &&
    input.activeTurn !== null &&
    input.permissionRequest === null &&
    !hasActiveToolCall(input.activeTurn)
  )
}

function getMessageContextUsage(message: acp.AnyMessage) {
  const update = getSessionUpdate(message)
  return update ? parseContextUsage(update) : null
}

function getPermissionRequest(message: acp.AnyMessage) {
  const params = getMessageField(message, "params")

  if (!isPermissionRequestMessage(message) || !isRecord(params)) {
    return null
  }

  return params as acp.RequestPermissionRequest
}

function resolveNextLiveSequence(turns: readonly SessionChatTurn[]) {
  return turns.reduce((sequence, turn) => Math.max(sequence, turn.sequence), -1) + 1
}

function buildLiveTurnId(promptRequestId: SessionHistoryTurn["promptRequestId"]) {
  return `live:${String(promptRequestId)}`
}

function resolvePromptRequestId(message: acp.AnyMessage) {
  const id = getMessageId(message)

  if (id !== null && isPromptMessage(message)) {
    return id
  }

  if (id !== null && (getMessageResult(message) || getMessageError(message))) {
    return id
  }

  return null
}

function getActiveTurn(turns: readonly SessionChatTurn[]) {
  const turn = turns.at(-1)

  return turn?.completedAt === null ? turn : null
}

function getTurnAgentText(turn: Pick<SessionHistoryTurn, "messages">) {
  return turn.messages.map((message) => getTextAgentMessageChunkText(message) ?? "").join("")
}

function hasTurnMessage(turns: readonly SessionChatTurn[], message: acp.AnyMessage) {
  if (getMessageId(message) === null) {
    return false
  }

  const fingerprint = buildMessageFingerprint(message)

  return turns.some((turn) =>
    turn.messages.some(
      (existingMessage) => buildMessageFingerprint(existingMessage) === fingerprint,
    ),
  )
}

/** Finds the turn waiting on one ACP permission request id so its response stays in-row. */
function findTurnWithPendingPermissionRequest(
  turns: readonly SessionChatTurn[],
  requestId: MessageId,
) {
  for (let turnIndex = turns.length - 1; turnIndex >= 0; turnIndex -= 1) {
    const turn = turns[turnIndex]
    let hasRequest = false
    let hasResponse = false

    for (const message of turn.messages) {
      if (getMessageId(message) !== requestId) {
        continue
      }

      if (isPermissionRequestMessage(message)) {
        hasRequest = true
      } else if (getMessageResult(message) || getMessageError(message)) {
        hasResponse = true
      }
    }

    if (hasRequest && !hasResponse) {
      return turn
    }
  }

  return null
}

function extractStopReason(message: acp.AnyMessage) {
  const result = getMessageResult(message)
  return typeof result?.stopReason === "string"
    ? (result.stopReason as SessionHistoryTurn["stopReason"])
    : null
}

function findPendingPermissionRequest(turns: readonly SessionChatTurn[]) {
  const resolvedRequestIds = new Set<MessageId>()
  const pendingRequests: SessionPermissionRequest[] = []

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

function resolveSessionChatStatus(
  session: DaemonSession,
  turns: readonly SessionChatTurn[],
  permissionRequest: SessionPermissionRequest | null,
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

/** Reactive session chat owner that merges loaded history with live daemon updates. */
export class SessionChat extends Sigma<SessionChatState> {
  #chunkFlushTimer: ReturnType<typeof setTimeout> | null = null
  #queuedChunks: QueuedSessionChatMessage[] = []

  constructor(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    super({
      connection: input.history.connection,
      hasMore: input.history.hasMore,
      nextCursor: input.history.nextCursor,
      session: input.session,
      summary: {
        activeTurnId: null,
        contextUsage: null,
        pendingPermissionRequest: null,
        showThinkingLabel: false,
        status: "idle",
      },
      turns: [],
    })

    this.syncLoadedData(input)
  }

  get hasEmptyTranscript() {
    return this.turns.length === 0 && !this.session.lastAgentMessage
  }

  get transcriptMessages() {
    return buildSessionChatTranscript({
      session: this.session,
      turns: this.turns,
    })
  }

  get repositoryLabel() {
    return getSessionRepositoryLabel(this.session)
  }

  get title() {
    return getSessionDisplayTitle(this.session)
  }

  /** Applies refreshed query data while preserving loaded older pages and local live messages. */
  syncLoadedData(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    const refreshedTurnsById = new Map(input.history.turns.map((turn) => [turn.turnId, turn]))
    const preservedLoadedTurns = this.turns.filter(
      (turn) => turn.source !== "live" && !refreshedTurnsById.has(turn.turnId),
    )
    const refreshedTurnIds = new Set(input.history.turns.map((turn) => turn.turnId))
    const hasPreservedOlderTurns = preservedLoadedTurns.some(
      (turn) => !refreshedTurnIds.has(turn.turnId),
    )
    const preservedHasMore = this.hasMore
    const preservedNextCursor = this.nextCursor
    const localMessages: { message: acp.AnyMessage; receivedAt: string }[] = []

    for (const turn of this.turns) {
      if (turn.source === "live") {
        for (const message of turn.messages) {
          localMessages.push({
            message,
            receivedAt: turn.completedAt ?? turn.startedAt,
          })
        }
        continue
      }

      const refreshedTurn = refreshedTurnsById.get(turn.turnId)
      if (!refreshedTurn || turn.source !== "merged") {
        continue
      }

      const existingAgentText = getTurnAgentText(turn)
      const refreshedAgentText = getTurnAgentText(refreshedTurn)
      if (
        !existingAgentText.startsWith(refreshedAgentText) ||
        existingAgentText.length === refreshedAgentText.length
      ) {
        continue
      }

      localMessages.push({
        message: createAgentMessageChunk(
          this.session.acpSessionId,
          existingAgentText.slice(refreshedAgentText.length),
        ),
        receivedAt: turn.completedAt ?? turn.startedAt,
      })
    }

    this.connection = input.history.connection
    this.hasMore = hasPreservedOlderTurns ? preservedHasMore : input.history.hasMore
    this.nextCursor = hasPreservedOlderTurns ? preservedNextCursor : input.history.nextCursor
    this.session = input.session
    this.turns.length = 0

    for (const turn of preservedLoadedTurns) {
      this.#mergeHistoryTurn(turn)
    }

    for (const turn of input.history.turns) {
      this.#mergeHistoryTurn(turn)
    }

    for (const localMessage of localMessages) {
      this.#mergeMessage(localMessage.message, localMessage.receivedAt)
    }

    this.#refreshTranscriptState()
  }

  /** Loads the next older history page and merges it ahead of the current transcript. */
  async loadOlderHistory() {
    if (!this.hasMore || this.nextCursor === null) {
      return
    }

    const history = await goddardSdk.session.history({
      id: this.session.id,
      cursor: this.nextCursor,
    })

    this.prependOlderHistory(history)
  }

  /** Merges one older history page ahead of the currently loaded transcript. */
  prependOlderHistory(history: GetSessionHistoryResponse) {
    const currentTurns = [...this.turns]
    const olderTurnIds = new Set(history.turns.map((turn) => turn.turnId))

    this.connection = history.connection
    this.hasMore = history.hasMore
    this.nextCursor = history.nextCursor
    this.turns.length = 0

    for (const turn of history.turns) {
      this.#mergeHistoryTurn(turn)
    }

    for (const turn of currentTurns) {
      if (!olderTurnIds.has(turn.turnId)) {
        this.#mergeHistoryTurn(turn)
      }
    }

    this.#refreshTranscriptState()
  }
  /** Applies one daemon-published ACP message through the live chunk batching queue. */
  receiveMessage(message: acp.AnyMessage) {
    const receivedAt = new Date().toISOString()
    const contextUsage = getMessageContextUsage(message)

    if (contextUsage) {
      this.session.contextUsage = contextUsage
      this.#refreshTranscriptState()
      return
    }

    if (isTextAgentMessageChunk(message)) {
      this.#queuedChunks.push({ message, receivedAt })
      this.#scheduleQueuedChunkFlush()
      return
    }

    this.#applyMessages([...this.#takeQueuedChunks(), { message, receivedAt }])
  }

  /** Applies one ACP message immediately, bypassing live chunk batching. */
  applyMessageNow(message: acp.AnyMessage, options: ApplySessionChatMessageOptions = {}) {
    const receivedAt = options.receivedAt ?? new Date().toISOString()
    const contextUsage = getMessageContextUsage(message)

    if (contextUsage) {
      this.session.contextUsage = contextUsage
      this.#refreshTranscriptState()
      return
    }

    this.#applyMessages([{ message, receivedAt }])
  }

  /** Flushes queued live text chunks into chat state. */
  flushReceivedMessages() {
    if (this.#queuedChunks.length === 0) {
      this.#takeQueuedChunks()
      return
    }

    this.#applyMessages(this.#takeQueuedChunks())
  }

  /** Applies a freshly returned session record without replacing the merged transcript. */
  syncSession(session: Immutable<DaemonSession>) {
    this.session = castDraft(session)
    this.#refreshTranscriptState()
  }

  /** Control whether the session is completed. */
  toggleCompletedHidden(completedHidden: boolean) {
    this.session.completedHidden = completedHidden
  }

  #mergeMessage(message: acp.AnyMessage, receivedAt: string) {
    if (hasTurnMessage(this.turns, message)) {
      return
    }

    const messageId = getMessageId(message)
    const promptRequestId = resolvePromptRequestId(message)
    const permissionTurn =
      messageId !== null && (getMessageResult(message) || getMessageError(message))
        ? findTurnWithPendingPermissionRequest(this.turns, messageId)
        : null
    const existingTurn =
      permissionTurn ??
      (promptRequestId === null
        ? getActiveTurn(this.turns)
        : (this.turns.find((turn) => turn.promptRequestId === promptRequestId) ?? null))

    if (existingTurn) {
      this.#applyMessageToTurn(existingTurn, message, receivedAt)
      return
    }

    const sequence = resolveNextLiveSequence(this.turns)
    const turn = this.#createLiveTurn(
      promptRequestId ?? `unattributed:${sequence}`,
      receivedAt,
      sequence,
    )

    this.#applyMessageToTurn(turn, message, receivedAt)
    this.turns.push(turn)
  }

  #applyMessages(messages: readonly QueuedSessionChatMessage[]) {
    if (messages.length === 0) {
      return
    }

    for (const { message, receivedAt } of messages) {
      this.#mergeMessage(message, receivedAt)
    }

    this.#refreshTranscriptState()
  }

  #cancelQueuedChunkFlush() {
    if (this.#chunkFlushTimer === null) {
      return
    }

    clearTimeout(this.#chunkFlushTimer)
    this.#chunkFlushTimer = null
  }

  #takeQueuedChunks() {
    this.#cancelQueuedChunkFlush()
    return this.#queuedChunks.splice(0, this.#queuedChunks.length)
  }

  #scheduleQueuedChunkFlush() {
    if (this.#chunkFlushTimer !== null) {
      return
    }

    this.#chunkFlushTimer = setTimeout(() => {
      this.flushReceivedMessages()
    }, liveChunkBatchIntervalMs)
  }

  #clearQueuedChunks() {
    this.#cancelQueuedChunkFlush()
    this.#queuedChunks.length = 0
  }

  #applyMessageToTurn(turn: SessionChatTurn, message: acp.AnyMessage, receivedAt: string) {
    if (turn.source === "history") {
      turn.source = "merged"
    }

    this.#insertTurnMessage(turn, message)
    this.#completeTurnWithMessage(turn, message, receivedAt)
    this.#rebuildTurnEvents(turn)
    turn.status = resolveTurnStatus(turn)
  }

  #mergeHistoryTurn(turn: SessionHistoryTurn) {
    const existingTurn = this.turns.find((currentTurn) => currentTurn.turnId === turn.turnId)

    if (!existingTurn) {
      this.turns.push(this.#normalizeTurn(turn, "history"))
      return
    }

    if (existingTurn.source === "live") {
      existingTurn.source = "merged"
    }

    existingTurn.sequence = turn.sequence
    existingTurn.promptRequestId = turn.promptRequestId
    existingTurn.startedAt = turn.startedAt
    existingTurn.completedAt ??= turn.completedAt
    existingTurn.completionKind ??= turn.completionKind
    existingTurn.stopReason ??= turn.stopReason
    existingTurn.inboxScope ??= turn.inboxScope
    existingTurn.inboxHeadline ??= turn.inboxHeadline

    for (const message of turn.messages) {
      this.#insertTurnMessage(existingTurn, message)
    }

    this.#rebuildTurnEvents(existingTurn)
    existingTurn.status = resolveTurnStatus(existingTurn)
  }

  #insertTurnMessage(turn: SessionChatTurn, message: acp.AnyMessage) {
    const fingerprint = getMessageId(message) === null ? null : buildMessageFingerprint(message)

    if (fingerprint !== null) {
      for (const existingMessage of turn.messages) {
        if (buildMessageFingerprint(existingMessage) === fingerprint) {
          return
        }
      }
    }

    turn.messages.push(message)
    turn.messages.sort(
      (left, right) =>
        getTurnMessageRank(left, turn.promptRequestId) -
        getTurnMessageRank(right, turn.promptRequestId),
    )
  }

  #completeTurnWithMessage(turn: SessionChatTurn, message: acp.AnyMessage, receivedAt: string) {
    if (getMessageId(message) !== turn.promptRequestId) {
      return
    }

    if (getMessageError(message)) {
      turn.completedAt ??= receivedAt
      turn.completionKind = "error"
      turn.status = "failed"
      return
    }

    if (getMessageResult(message)) {
      turn.completedAt ??= receivedAt
      turn.completionKind = "result"
      turn.stopReason = extractStopReason(message)
    }
  }

  #normalizeTurn(turn: SessionHistoryTurn, source: SessionChatTurn["source"]) {
    const events: SessionChatTurnEvent[] = []
    const normalized = {
      ...turn,
      messages: [...turn.messages],
      events,
      source,
      status: resolveTurnStatus(turn),
    } satisfies SessionChatTurn

    this.#rebuildTurnEvents(normalized)
    normalized.status = resolveTurnStatus(normalized)

    return normalized
  }

  #createLiveTurn(
    promptRequestId: SessionHistoryTurn["promptRequestId"],
    receivedAt: string,
    sequence: number,
  ) {
    return this.#normalizeTurn(
      {
        turnId: buildLiveTurnId(promptRequestId),
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

  #rebuildTurnEvents(turn: SessionChatTurn) {
    const permissionRequestIds = new Set<MessageId>()

    for (const message of turn.messages) {
      if (isPermissionRequestMessage(message)) {
        permissionRequestIds.add(getMessageId(message)!)
      }
    }

    turn.events.length = 0

    for (const [messageIndex, message] of turn.messages.entries()) {
      const id = getMessageId(message)

      if (isPromptMessage(message) && id === turn.promptRequestId) {
        turn.events.push({
          kind: "prompt",
          messageIndex,
          promptRequestId: turn.promptRequestId,
        })
        continue
      }

      const update = getSessionUpdate(message)
      if (update && typeof update.sessionUpdate === "string") {
        const plan = parsePlanEvent(update)

        turn.events.push(
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

      const permissionRequest = getPermissionRequest(message)
      if (permissionRequest && id !== null) {
        turn.events.push({
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
        (getMessageResult(message) || getMessageError(message))
      ) {
        turn.events.push({
          kind: "permissionResponse",
          messageIndex,
          requestId: id,
        })
        continue
      }

      if (id === turn.promptRequestId && (getMessageResult(message) || getMessageError(message))) {
        turn.events.push({
          kind: "turnCompletion",
          messageIndex,
          completionKind: getMessageError(message) ? "error" : "result",
          stopReason: extractStopReason(message),
        })
      }
    }
  }

  #refreshTranscriptState() {
    this.#syncSummary()
  }

  #syncSummary() {
    const activeTurn = getActiveTurn(this.turns)
    const permissionRequest = findPendingPermissionRequest(this.turns)
    const status = resolveSessionChatStatus(this.session, this.turns, permissionRequest)

    this.summary = {
      activeTurnId: activeTurn?.turnId ?? null,
      contextUsage: this.session.contextUsage,
      pendingPermissionRequest: permissionRequest,
      showThinkingLabel: shouldShowThinkingLabel({ activeTurn, permissionRequest, status }),
      status,
    }
  }

  onSetup() {
    let active = true
    let unsubscribe: (() => void) | null = null

    void goddardSdk.session
      .subscribe({ id: this.session.id }, (message) => {
        if (active) {
          this.receiveMessage(message)
        }
      })
      .then(
        (nextUnsubscribe) => {
          if (active) {
            unsubscribe = nextUnsubscribe
          } else {
            nextUnsubscribe()
          }
        },
        (error) => {
          if (active) {
            console.error("Failed to subscribe to session chat updates.", error)
          }
        },
      )

    return [
      () => {
        active = false
        this.#clearQueuedChunks()
        unsubscribe?.()
        unsubscribe = null
      },
    ]
  }
}

export interface SessionChat extends SessionChatState {}
