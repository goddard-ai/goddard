import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse, SessionHistoryTurn } from "@goddard-ai/sdk"
import hashSum from "hash-sum"
import { Sigma } from "preact-sigma"

import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "~/sessions/presentation.ts"
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
  pendingPermissionRequest: Extract<SessionChatTurnEvent, { kind: "permissionRequest" }> | null
  status: SessionChatStatus
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

/** Reactive session chat owner that merges loaded history with live daemon updates. */
export class SessionChat extends Sigma<SessionChatState> {
  constructor(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    super({
      connection: input.history.connection,
      hasMore: input.history.hasMore,
      nextCursor: input.history.nextCursor,
      session: input.session,
      summary: {
        activeTurnId: null,
        pendingPermissionRequest: null,
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

  /** Applies refreshed query data while preserving live messages already received locally. */
  syncLoadedData(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    const localMessages: { message: acp.AnyMessage; receivedAt: string }[] = []

    for (const turn of this.turns) {
      if (turn.source === "history") {
        continue
      }

      for (const message of turn.messages) {
        localMessages.push({
          message,
          receivedAt: turn.completedAt ?? turn.startedAt,
        })
      }
    }

    this.connection = input.history.connection
    this.hasMore = input.history.hasMore
    this.nextCursor = input.history.nextCursor
    this.session = input.session
    this.turns.length = 0

    for (const turn of input.history.turns) {
      this.turns.push(this.#normalizeTurn(turn, "history"))
    }

    for (const localMessage of localMessages) {
      this.#mergeMessage(localMessage.message, localMessage.receivedAt)
    }

    this.#refreshTranscriptState()
  }

  /** Applies one daemon-published ACP message to the chat state. */
  applyMessage(message: acp.AnyMessage, options: ApplySessionChatMessageOptions = {}) {
    const receivedAt = options.receivedAt ?? new Date().toISOString()

    this.#mergeMessage(message, receivedAt)
    this.#refreshTranscriptState()
  }

  /** Applies a freshly returned session record without replacing the merged transcript. */
  syncSession(session: DaemonSession) {
    this.session = session
    this.#refreshTranscriptState()
  }

  #mergeMessage(message: acp.AnyMessage, receivedAt: string) {
    const promptRequestId = this.#promptRequestIdFromMessage(message)
    const existingTurn =
      promptRequestId === null
        ? this.#newestRunningTurn()
        : (this.turns.find((turn) => turn.promptRequestId === promptRequestId) ?? null)

    if (existingTurn) {
      this.#applyMessageToTurn(existingTurn, message, receivedAt)
      return
    }

    const sequence = this.#nextLiveSequence()
    const turn = this.#createLiveTurn(
      promptRequestId ?? `unattributed:${sequence}`,
      receivedAt,
      sequence,
    )

    this.#applyMessageToTurn(turn, message, receivedAt)
    this.turns.push(turn)
  }

  #applyMessageToTurn(turn: SessionChatTurn, message: acp.AnyMessage, receivedAt: string) {
    if (turn.source === "history") {
      turn.source = "merged"
    }

    this.#insertTurnMessage(turn, message)
    this.#completeTurnFromMessage(turn, message, receivedAt)
    this.#rebuildTurnEvents(turn)
    turn.status = this.#turnStatus(turn)
  }

  #insertTurnMessage(turn: SessionChatTurn, message: acp.AnyMessage) {
    const fingerprint = this.#messageFingerprint(message)

    for (const existingMessage of turn.messages) {
      if (this.#messageFingerprint(existingMessage) === fingerprint) {
        return
      }
    }

    turn.messages.push(message)
    turn.messages.sort(
      (left, right) =>
        this.#turnMessageRank(left, turn.promptRequestId) -
        this.#turnMessageRank(right, turn.promptRequestId),
    )
  }

  #completeTurnFromMessage(turn: SessionChatTurn, message: acp.AnyMessage, receivedAt: string) {
    if (this.#messageId(message) !== turn.promptRequestId) {
      return
    }

    if (this.#messageError(message)) {
      turn.completedAt ??= receivedAt
      turn.completionKind = "error"
      turn.status = "failed"
      return
    }

    if (this.#messageResult(message)) {
      turn.completedAt ??= receivedAt
      turn.completionKind = "result"
      turn.stopReason = this.#extractStopReason(message)
    }
  }

  #normalizeTurn(turn: SessionHistoryTurn, source: SessionChatTurn["source"]) {
    const events: SessionChatTurnEvent[] = []
    const normalized = {
      ...turn,
      messages: [...turn.messages],
      events,
      source,
      status: this.#turnStatus(turn),
    } satisfies SessionChatTurn

    this.#rebuildTurnEvents(normalized)
    normalized.status = this.#turnStatus(normalized)

    return normalized
  }

  #createLiveTurn(
    promptRequestId: SessionHistoryTurn["promptRequestId"],
    receivedAt: string,
    sequence: number,
  ) {
    return this.#normalizeTurn(
      {
        turnId: this.#liveTurnId(promptRequestId),
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
      if (this.#isPermissionRequestMessage(message)) {
        permissionRequestIds.add(this.#messageId(message)!)
      }
    }

    turn.events.length = 0

    for (const [messageIndex, message] of turn.messages.entries()) {
      const id = this.#messageId(message)

      if (this.#isPromptMessage(message) && id === turn.promptRequestId) {
        turn.events.push({
          kind: "prompt",
          messageIndex,
          promptRequestId: turn.promptRequestId,
        })
        continue
      }

      const update = this.#sessionUpdate(message)
      if (update && typeof update.sessionUpdate === "string") {
        const plan = this.#parsePlanEvent(update)

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

      const permissionRequest = this.#permissionRequestFromMessage(message)
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
        (this.#messageResult(message) || this.#messageError(message))
      ) {
        turn.events.push({
          kind: "permissionResponse",
          messageIndex,
          requestId: id,
        })
        continue
      }

      if (
        id === turn.promptRequestId &&
        (this.#messageResult(message) || this.#messageError(message))
      ) {
        turn.events.push({
          kind: "turnCompletion",
          messageIndex,
          completionKind: this.#messageError(message) ? "error" : "result",
          stopReason: this.#extractStopReason(message),
        })
      }
    }
  }

  #refreshTranscriptState() {
    this.turns.sort((left, right) => this.#compareTurns(left, right))
    this.#syncSummary()
  }

  #syncSummary() {
    const activeTurn = this.#newestRunningTurn()
    const permissionRequest = this.#pendingPermissionRequest()

    this.summary = {
      activeTurnId: activeTurn?.turnId ?? null,
      pendingPermissionRequest: permissionRequest,
      status: this.#sessionChatStatus(permissionRequest),
    }
  }

  #pendingPermissionRequest() {
    const resolvedRequestIds = new Set<MessageId>()
    const pendingRequests: SessionPermissionRequest[] = []

    for (const turn of this.turns) {
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

  #sessionChatStatus(permissionRequest: SessionPermissionRequest | null) {
    if (
      this.session.status === "blocked" ||
      this.session.permissions !== null ||
      permissionRequest
    ) {
      return "blocked"
    }

    if (this.turns.some((turn) => turn.status === "running")) {
      return "running"
    }

    if (this.session.status === "error") {
      return "failed"
    }

    if (this.session.status === "cancelled") {
      return "cancelled"
    }

    if (this.session.status === "done" || this.session.status === "archived") {
      return "completed"
    }

    return "idle"
  }

  #newestRunningTurn() {
    for (let index = this.turns.length - 1; index >= 0; index -= 1) {
      if (this.turns[index].completedAt === null) {
        return this.turns[index]
      }
    }

    return null
  }

  #nextLiveSequence() {
    return this.turns.reduce((sequence, turn) => Math.max(sequence, turn.sequence), -1) + 1
  }

  #turnStatus(turn: Pick<SessionHistoryTurn, "completedAt" | "completionKind" | "stopReason">) {
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

  #compareTurns(left: SessionChatTurn, right: SessionChatTurn) {
    if (left.sequence !== right.sequence) {
      return left.sequence - right.sequence
    }

    if (left.startedAt !== right.startedAt) {
      return left.startedAt.localeCompare(right.startedAt)
    }

    return left.turnId.localeCompare(right.turnId)
  }

  #turnMessageRank(
    message: acp.AnyMessage,
    promptRequestId: SessionHistoryTurn["promptRequestId"],
  ) {
    const id = this.#messageId(message)

    if (this.#isPromptMessage(message) && id === promptRequestId) {
      return 0
    }

    if (id === promptRequestId && (this.#messageResult(message) || this.#messageError(message))) {
      return 2
    }

    return 1
  }

  #promptRequestIdFromMessage(message: acp.AnyMessage) {
    const id = this.#messageId(message)

    if (id !== null && this.#isPromptMessage(message)) {
      return id
    }

    if (id !== null && (this.#messageResult(message) || this.#messageError(message))) {
      return id
    }

    return null
  }

  #messageField(message: acp.AnyMessage, key: string) {
    if (!this.#isRecord(message)) {
      return null
    }

    return (message as Record<string, unknown>)[key]
  }

  #messageId(message: acp.AnyMessage) {
    const id = this.#messageField(message, "id")

    return typeof id === "string" || typeof id === "number" ? id : null
  }

  #messageMethod(message: acp.AnyMessage) {
    const method = this.#messageField(message, "method")

    return typeof method === "string" ? method : null
  }

  #messageResult(message: acp.AnyMessage) {
    const result = this.#messageField(message, "result")

    return this.#isRecord(result) ? (result as Record<string, unknown>) : null
  }

  #messageError(message: acp.AnyMessage) {
    const error = this.#messageField(message, "error")

    return this.#isRecord(error) ? (error as Record<string, unknown>) : null
  }

  #isPromptMessage(message: acp.AnyMessage) {
    return (
      this.#messageMethod(message) === acp.AGENT_METHODS.session_prompt &&
      this.#messageId(message) !== null
    )
  }

  #isPermissionRequestMessage(message: acp.AnyMessage) {
    return (
      this.#messageMethod(message) === acp.CLIENT_METHODS.session_request_permission &&
      this.#messageId(message) !== null
    )
  }

  #sessionUpdate(message: acp.AnyMessage) {
    const params = this.#messageField(message, "params")

    if (
      this.#messageMethod(message) !== acp.CLIENT_METHODS.session_update ||
      !this.#isRecord(params)
    ) {
      return null
    }

    const update = (params as Record<string, unknown>).update

    return this.#isRecord(update) ? (update as Record<string, unknown>) : null
  }

  #parsePlanEvent(update: Record<string, unknown>) {
    return update.sessionUpdate === "plan" && Array.isArray(update.entries)
      ? (update as acp.Plan)
      : null
  }

  #permissionRequestFromMessage(message: acp.AnyMessage) {
    const params = this.#messageField(message, "params")

    if (!this.#isPermissionRequestMessage(message) || !this.#isRecord(params)) {
      return null
    }

    return params as acp.RequestPermissionRequest
  }

  #extractStopReason(message: acp.AnyMessage) {
    const result = this.#messageResult(message)
    return typeof result?.stopReason === "string"
      ? (result.stopReason as SessionHistoryTurn["stopReason"])
      : null
  }

  #messageFingerprint(message: acp.AnyMessage) {
    return hashSum(message)
  }

  #liveTurnId(promptRequestId: SessionHistoryTurn["promptRequestId"]) {
    return `live:${String(promptRequestId)}`
  }

  #isRecord(value: unknown) {
    return typeof value === "object" && value !== null
  }

  onSetup() {
    let active = true
    let unsubscribe: (() => void) | null = null

    void goddardSdk.session
      .subscribe({ id: this.session.id }, (message) => {
        if (active) {
          this.applyMessage(message)
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
        unsubscribe?.()
        unsubscribe = null
      },
    ]
  }
}

export interface SessionChat extends SessionChatState {}
